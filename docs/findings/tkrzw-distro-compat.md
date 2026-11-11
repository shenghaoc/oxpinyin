# Debian now ships libpinyin on tkrzw; Ubuntu's tkrzw silently corrupts records

Status: **record** (cross-distro matrix measured 2026-08-29; tkrzw
1.0.34 measured 2026-10-01 and 2026-10-05). tkrzw 1.0.34 (`9ee8416`,
2026-09-20) fixes defect 1 upstream and leaves defect 2. Everything below
about defect 1 therefore describes 1.0.33 and earlier. tkrzw is
oxpinyin's default backend and reference cell. Development builds and CI use
the repository's vcpkg overlay port, whose
source revision is in `vcpkg-overlays/tkrzw/pin.json`. Debian release packaging,
shipping-size measurements and oracle/reference images retain Debian's library.
The former Ubuntu-only `distro-probe.sh` has been removed; references below
record the historical experiments.

Ubuntu's packaged libtkrzw breaks tkrzw's pointer-identity protocol.
`./configure && make` on the same sources is correct. This note states
why, and how far the fault reaches.

**Debian has switched `libpinyin` to the tkrzw backend** (`2.11.91-1`,
unstable/testing, 2026-08-12). These faults are therefore under a shipped
input method's user dictionary, not only a command-line tool. Debian's
tkrzw package is healthy (measured `1.0.32-1+b2`), so that shipped
combination is safe today. The exposure is Ubuntu. Its tkrzw
(`1.0.32-1build1`) is broken on both defects below, and it syncs
`libpinyin` from Debian. See "Does anything actually ship against this?"
for the measured per-distro backend matrix.

**There are two independent defects, with two different causes.** Both
come from build flags that Ubuntu applies to every package and Debian
applies to none. Both are silent, and neither fix repairs the other:

| | Cause | Breaks | Symptom |
| --- | --- | --- | --- |
| **1** | `-flto` | `RecordProcessor::NOOP` / `REMOVE` — *data* | `Remove()` stores a tombstone instead of deleting; a NOOP processor overwrites the record; `Rebuild` mis-counts |
| **2** | `-Wl,-Bsymbolic-functions` | the key comparators — *functions* | TreeDBM records comparator type 255 and can never reopen the file |

This matters for the fix. Ubuntu bug [LP #2142937][lp] carries a patch
that disables LTO. That patch resolves defect 1 and leaves defect 2
unchanged. See "Reported" below.

[lp]: https://bugs.launchpad.net/ubuntu/+source/tkrzw/+bug/2142937

## What was measured

Every Ubuntu release that carries tkrzw, checked with
`tools/tkrzw/distro-probe.sh`:

| Ubuntu release | Support | tkrzw package | `remove` | `.tkt` round-trip |
| --- | --- | --- | --- | --- |
| 24.04 LTS (noble) | supported | `1.0.27-1.1build1` | **tombstone** | **unreadable** |
| 25.10 (questing) | EOL July 2026 | `1.0.32-1` | **tombstone** | **unreadable** |
| 26.04.1 LTS (resolute) | supported | `1.0.32-1build1` | **tombstone** | **unreadable** |
| 26.10 (stonking, devel) | development | `1.0.32-1build1` | — | — |

Three consecutive releases, two upstream versions and three different
Debian revision strings all have both defects. The development series
carries resolute's binary unchanged. At the date of this note, no fix is
in Ubuntu.

Note questing's row. Its version string is `1.0.32-1`, *identical* to
Debian's, with no `buildN` suffix, because it is a direct source sync and
not a no-change rebuild. The source and version string are the same, but
the binary differs, because Ubuntu built it. The `buildN` suffix does not
mark an affected package. A build by Ubuntu does.

And the same source built elsewhere:

| Build | `remove` | `.tkt` round-trip |
| --- | --- | --- |
| upstream `1.0.27`, `./configure && make` | ok | ok |
| Debian source pkg `1.0.27-1.1`, Debian flags | ok | ok |
| Debian source pkg `1.0.27-1.1`, Ubuntu flags | **tombstone** | **unreadable** |
| Debian source pkg `1.0.32-1`, Debian flags | ok | ok |
| Debian source pkg `1.0.32-1`, Ubuntu flags | **tombstone** | **unreadable** |

`1.0.32-1` is the important version. Debian uploaded it to unstable,
and Ubuntu ships it in questing, resolute and devel. Built with Ubuntu's
flags, it reproduces the shipped failure exactly. Built with Debian's
flags, from the same tarball on the same machine, it is clean.

"unreadable" is literal. `tkrzw_dbm_util create x.tkt` succeeds, and
every later open of that file by the same binary fails with
`BROKEN_DATA_ERROR: invalid_key_comparator`. Defect 2 does not affect
SkipDBM (`.tks`), which stores no comparator. Every DBM type is exposed to
defect 1.

## The bisection

The bisection uses four real rebuilds of Debian's `tkrzw_1.0.32-1` source
package, all with `DEB_VENDOR=Ubuntu`. Only the two flags vary:

| Build | LTO | `-Bsymbolic-functions` | `remove` | `.tkt` | comparator GOT relocs |
| --- | --- | --- | --- | --- | --- |
| A — stock Ubuntu | on | on | **tombstone** | **unreadable** | 0 |
| B — `optimize=-lto` (the LP patch) | off | on | ok | **unreadable** | 0 |
| C — strip the linker flag only | on | off | **tombstone** | ok | 18 |
| D — strip both | off | off | ok | ok | 18 |
| Debian vendor, for reference | off | off | ok | ok | 18 |

The two columns are independent. LTO alone controls `remove`.
`-Bsymbolic-functions` alone controls the comparator. Row D and the
Debian reference agree. That agreement shows that nothing else in
Ubuntu's flag set is involved. `-Bsymbolic-functions` is necessary and
sufficient for defect 2 only. Row C shows that it does not explain
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
only. Row B above confirms this: it passes `remove` and fails `.tkt`. LTO
breaks the sentinels, and the binary shows the mechanism. These are the
counts of each backing literal inside `libtkrzw.so`:

```
A stock Ubuntu (LTO + Bsym)    NOOP=10   REMOVE=11
B LP patch  (no LTO, Bsym)     NOOP=1    REMOVE=1
C LTO, no Bsym                 NOOP=10   REMOVE=11
D neither                      NOOP=1    REMOVE=1
```

Without LTO there is exactly one copy of each. The exported `NOOP`
object and every comparison site therefore agree. With LTO, GCC's
partitioning gives most partitions their own copy (ten and eleven
copies). The address that a comparison site was compiled against is then
not the address that the exported object carries, and
`value.data() == NOOP.data()` fails. `Remove` then writes the REMOVE
sentinel as the record's value and reports success.

## It is not only the CLI

Both defects reach any *client* of the library, which makes them
dangerous for a backend like this one. Defect 1 reaches further: row (d)
below calls plain `dbm.Remove(key)` and passes no sentinel across the
boundary. The output below is from a client compiled against each build
(a scratch identity probe, recorded evidence):

```
──── client compiled against the Debian-flags libtkrzw ────
(a) default comparator (nullptr)        : key_comp_type=1   reopen=OK
(b) client passes LexicalKeyComparator  : key_comp_type=1   reopen=OK
(c) NOOP-returning processor            : value now "v"                    OK
(d) Remove()                            : OK (record gone)
(e) Rebuild()                           : OK

──── client compiled against the Ubuntu-flags libtkrzw ────
(a) default comparator (nullptr)        : key_comp_type=1   reopen=OK
(b) client passes LexicalKeyComparator  : key_comp_type=255 reopen=BROKEN_DATA_ERROR: invalid_key_comparator
(c) NOOP-returning processor            : value now "\x00\xbe\xef\x02\x11" CORRUPTED
(d) Remove()                            : BROKEN (record still present, value="\x00\xde\xad\x02\x11")
(e) Rebuild()                           : CANCELED_ERROR
```

Row (a) is the safe case. A `key_comparator` left at its `nullptr`
default keeps the decision inside the library, where both sides of the
comparison are the same copy. `oxpinyin-store` installs no comparator, so
defect 2 does not reach its files. `oxpinyin-store` *is* exposed to rows
(c)–(e), and they are silent. A `Remove` that stores the sentinel as the
value looks like a successful write.

## What decides it, per distro

Each defect depends on one bit of the distro's defaults. Primary sources
give both bits, with no tkrzw install.

| Distro | Source | `-Bsymbolic-functions` | LTO by default |
| --- | --- | --- | --- |
| Ubuntu | `Dpkg::Vendor::Ubuntu:156` | **yes** | **yes** |
| Debian | `Dpkg::Vendor::Debian` | no | no (opt-in per package) |
| Devuan, PureOS | their `Dpkg::Vendor::*` | no | no |
| Arch | `/etc/makepkg.conf` (`pacman 7.1.0.r9`) | no | **yes** (`OPTIONS=(... lto)`) |
| Fedora | `redhat-rpm-config` | no | **yes** globally, but `tkrzw.spec` sets `%_lto_cflags %{nil}` |
| RHEL 10 (via EPEL) | EPEL, Fedora spec lineage | no | no (`tkrzw.spec` opts out); not in RHEL's own repos |

The dpkg figures are from dpkg's own git at tag **1.23.7**, the exact
dpkg that the `debian:sid-slim` image reports. So this is Debian's file,
not Ubuntu's copy of it. Arch's full default `LDFLAGS` are

```
-Wl,-O1 -Wl,--sort-common -Wl,--as-needed -Wl,-z,relro -Wl,-z,now -Wl,-z,pack-relative-relocs
```

Its `-fno-plt` is not a hazard here: it changes how calls are routed,
not how addresses are taken.

**Debian is clean on both defects. Arch is clean on defect 2 but exposed
to defect 1**, because Arch enables LTO globally through
`OPTIONS=(... lto)`. Both results are measured (see "The matrix,
measured" below):

- Debian testing probes healthy.
- Arch packages no tkrzw to probe. A source build under its own flags
  reproduces defect 1 and not defect 2, as the flag bits predict.

The Launchpad reporter ran Debian and got the same clean result on
`1.0.32-1+b1` from trixie.

A package can still add either flag itself. tkrzw's package does not.
`debian/rules` never sets `LDFLAGS`: its only two mentions are a
commented-out `DEB_LDFLAGS_MAINT_APPEND`. The file is byte identical from
`1.0.27-1.1` through `1.0.32-1`.

## Does anything actually ship against this?

This section answers one question: has a distro switched libpinyin from
Berkeley DB to tkrzw, where these faults would affect a user's dictionary
and not only a command-line tool?

**Yes. Debian has, from 2026-08-12.** The facts below are measured, not
inferred:

- `libpinyin 2.11.91-1`, in unstable and testing (sid, forky), sets
  `--with-dbm=Tkrzw` in `debian/rules` and Build-Depends `libtkrzw-dev`.
- Its shipped `libpinyin.so.15` links `libtkrzw.so.1` and depends on
  `libtkrzw1t64 (>= 1.0.32)`.
- The changelog says "Switch from BerkeleyDB to Tkrzw" (Closes: #1119204,
  #993415). `debian/NEWS` warns that after "the engine switch from
  BerkeleyDB to Tkrzw ... all previous user data will be lost after the
  upgrade."

Upstream added the Tkrzw option in 2.11.91 only. Its `configure.ac` still
defaults to `DBM="BerkeleyDB"`, and its `--with-dbm` help string names
only "BerkeleyDB or KyotoCabinet". Debian is therefore the first distro to
select Tkrzw, by its own choice and not through an upstream default.
Debian stable is not affected: trixie and bookworm carry `2.8.1-1` on
Berkeley DB. But the switch is released into a rolling suite.

The full backend matrix was measured 2026-08-29. The method read each
distro's installed `libpinyin.so` `DT_NEEDED` entries
(`readelf`/`objdump`) for the directly linked storage library. Each
build recipe corroborates the result (`debian/rules`, Fedora
`libpinyin.spec`, Arch `PKGBUILD`, nixpkgs `package.nix`):

| Distro | libpinyin | DBM backend | evidence |
| --- | --- | --- | --- |
| Debian sid / forky | `2.11.91-1` | **Tkrzw** | links `libtkrzw.so.1`; `--with-dbm=Tkrzw` |
| Debian trixie / bookworm | `2.8.1-1` | Berkeley DB | pre-switch upload |
| Ubuntu 26.04 LTS | `2.10.3-1` | Berkeley DB | links `libdb-5.3.so` |
| Fedora Rawhide | `2.11.91` | Kyoto Cabinet | spec `--with-dbm=KyotoCabinet` |
| RHEL 10 (EPEL) | `2.8.1-9.el10` | Kyoto Cabinet | links `libkyotocabinet.so.16` |
| Arch | `2.10.3` | Kyoto Cabinet | PKGBUILD `--with-dbm=KyotoCabinet` |
| openSUSE Tumbleweed | `2.10.3` | Kyoto Cabinet | links `libkyotocabinet.so.16` |
| NixOS (nixpkgs) | `2.11.91` | Kyoto Cabinet | `package.nix --with-dbm=KyotoCabinet` |

There are three families and three backends. The RPM/Arch/Nix world is
on Kyoto Cabinet. The `.deb` world is on Berkeley DB, except Debian's
rolling suites, which are on tkrzw. No *shipped* cell is on tkrzw **and**
broken at the same time, at the date of this note. Debian's own tkrzw is
healthy (see the tkrzw matrix later in this note). A Debian sid user
therefore gets the switch without the defects, and loses only the old
dictionary that the NEWS warns about.

The dangerous cell is empty only because of timing: **Ubuntu's tkrzw is
broken on both defects, and Ubuntu tracks Debian.** The Ubuntu release
that syncs `libpinyin 2.11.91`'s `--with-dbm=Tkrzw` onto Ubuntu's own
libtkrzw puts defect 1 on every user dictionary. A `Remove` then silently
stores the sentinel and does not delete. The switch is already made
upstream of Ubuntu. Only Ubuntu's release cadence delays it.

The exposure is not what the file layout suggests:

- **The comparator fault does not reach libpinyin**, although two of its
  five tkrzw backends use TreeDBM. Its `src/` has no `OpenAdvanced`, no
  `TuningParameters` and no `key_comparator`. So the comparator stays
  `nullptr` and is resolved inside libtkrzw, where both sides of the
  comparison are the same copy (row (a) of the probe).
- **The sentinel fault does reach libpinyin.** Three `Remove()` call sites
  would leave the record in place, with the five-byte `REMOVE` sentinel
  as its value: `ngram_tkrzwdb.cpp:140` (`Bigram::remove`),
  `punct_table_tkrzwdb.cpp:157` and `flexible_ngram_tkrzwdb.h:263` (the
  user n-gram). That is user-dictionary data.
- The NOOP-returning processors are safe as written.
  `KeyCollectProcessor` and `FlexibleKeyCollectProcessor` both run under
  `ProcessEach(&processor, false)`, and the read-only traversal never
  writes a return value back.

This shows the general shape of the hazard. The sentinel fault is not a
client/library boundary problem. `dbm.Remove(key)` passes no sentinel
across any boundary and still corrupts, because LTO duplicated the
literal *inside* libtkrzw. Any consumer is exposed, in any language
binding, whether or not it names a sentinel. The comparator fault is
narrower. It needs a caller that names a built-in comparator itself. That
is why `tkrzw_dbm_util` triggers it and libpinyin does not.

## What this means for us

tkrzw is the default backend and the reference cell. `build.rs` requires
`pkg-config` to find tkrzw. It does **not** verify the origin or health of
the tkrzw that `pkg-config` finds, so a broken distro `libtkrzw-dev` on
`PKG_CONFIG_PATH` would link. development builds therefore use the repository's vcpkg overlay port.
A result against a defective libtkrzw is invalid
([verification runbook §9](../runbooks/verification.md#9-sound-tkrzw-library)).

The quickest check on a candidate library needs no build and no test
database:

```sh
readelf -rW /usr/lib/*/libtkrzw.so.1 | grep -c KeyComparator   # 0 = defect 2
```

`tools/tkrzw/distro-probe.sh` runs that check and a write-then-read round
trip. It exits non-zero on a broken build. The store suite's
`tkrzw_library_honours_its_sentinel_protocol` checks defect 1 on each DBM
class the backend opens.

## Reported

Ubuntu tracks the fault as [LP #2142937][lp]. Georgi Georgiev filed it
on 2026-02-28 against `src:tkrzw`, from the HashDBM `remove` symptom on
Ubuntu 25.10. The report correctly identifies the fault as
Ubuntu-specific and shows Debian trixie's `1.0.32-1+b1` behaving
correctly.

Know two facts before adding to the report:

- It is still New / Undecided / Unassigned after six months.
- It was filed against **25.10, which reached end of life in July 2026**.
  An Ubuntu bug whose only reproducer is an EOL interim release is easy
  to leave alone.

The measurements above answer the second point. The same faults are
present, unchanged, on 24.04 LTS and 26.04 LTS, and the development
series ships resolute's binary. So no Ubuntu release has a fix. The
report needs that fact, not a new explanation of the mechanism.

**The patch attached there is incomplete.** It sets

```make
export DEB_BUILD_MAINT_OPTIONS = hardening=+all optimize=-lto
```

This is row B of the bisection: `remove` is fixed, but the library still
cannot read any TreeDBM file that it writes. The complete fix needs both
lines:

```make
export DEB_BUILD_MAINT_OPTIONS = hardening=+all optimize=-lto
export DEB_LDFLAGS_MAINT_STRIP = -Wl,-Bsymbolic-functions
```

That is row D, which matches the Debian reference build exactly.

Both lines are no-ops on Debian, which enables neither flag. Their
natural place is therefore Debian's `debian/rules`, from which Ubuntu
syncs. Ubuntu then carries no permanent delta. Two facts limit that
route:

- `src:tkrzw` is orphaned in Debian (`Maintainer: Debian QA Group
  <packages@qa.debian.org>`, with Boyuan Yang doing QA uploads through
  `1.0.32-1`). It may need an NMU or a merge request against
  `salsa.debian.org/debian/tkrzw`.
- A sync reaches no released Ubuntu LTS. 24.04 and 26.04 need SRUs
  through the Launchpad bug in any case.

The libpinyin sync is real. Boyuan Yang, the same maintainer who does
tkrzw's QA uploads, switched Debian's `libpinyin` to `--with-dbm=Tkrzw` in
`2.11.91-1` (unstable, 2026-08-12). One archive therefore holds a
libtkrzw that is healthy in Debian and broken in Ubuntu, and a libpinyin
that writes user dictionaries through it. A fix to the tkrzw build before
Ubuntu's next libpinyin sync keeps defect 1 off those dictionaries. After
the sync, the Launchpad bug is about user data, not only a command-line
tool.

## Upstream

Upstream fixed defect 1 on 2026-09-20, in `9ee8416` (release 1.0.34),
by neither remedy proposed below. The check is still
`value.data() == NOOP.data()`, an address identity. The change is that a
named array, not a string literal, backs each sentinel. So there is
exactly one address to compare against. That is enough: an LTO build of
1.0.34 is healthy. Defect 2 is not fixed.

Both defects come from the same design decision: tkrzw identifies a value
by an address that C++ guarantees only under default ELF interposition
and a single definition. A distro flag, LTO partitioning, a static link
of one side or `dlopen` with `RTLD_LOCAL` each break it, silently and on
disk. On Windows, each module gets its own copy of an inline function by
default, so the identity is unlikely to hold at all.

A move of the comparators out of the header does not fix defect 2. A
function defined only in the shared library still fails under
`-Bsymbolic-functions`. The library binds to its own definition, while
the client's address-taking yields a canonical PLT entry in the client.
No design that identifies a comparator by its address is safe. A one-byte
enum in `TuningParameters` beside the pointer costs nothing and is
immune. The on-disk format is already an enum byte, so only the
derivation from a pointer has to change. The sentinels need the same
treatment: a tagged return, or a comparison on contents, not on `data()`.

Report this upstream separately from the Ubuntu bug, because it is a
different request. It is not a Rust-mechanism divergence, so it does not
belong in `upstream-divergences.md`.

## The matrix, measured

The matrix was run on 2026-08-29 with `tools/tkrzw/distro-probe.sh`, with
one rolling container per distro across the three packaging families
(podman; `:latest` / `testing` tags, deliberately unpinned). Defect 1 is
the LTO/`remove` sentinel. Defect 2 is the
`-Bsymbolic-functions`/comparator.

The tags are rolling by design, because the newest toolchain that a
distro ships is a property under test. For reproducibility, the images
behind this matrix and the backend matrix resolved on 2026-08-29 to these
digests:

- `ubuntu:26.04` — `sha256:2260313b31c8c011cd2eebe728008efac1b3982be73eb71348ea2648d2c0e09b`
- `debian:testing` — `sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9`
- `fedora:rawhide` — `sha256:029fe4c775d759de3de7ddb3c86f86e32213358bfb2e338e610b01c37da7d6be`
- `redhat/ubi10:latest` — `sha256:bc5a42833e4c84dbf7a29bcd4a0be414addad69e16210c2f0eb73986b356793c`
- `archlinux:latest` — `sha256:4bf33b21a715aac0b48ce6e9eaed4782a898eae96f88f5da3635572129c2584a`
- `opensuse/tumbleweed:latest` — `sha256:b4c13ab3c6225867da7cbf3191a9417cfa5bfc8cdc41d33e115d0ae1c15f44f7`

| Family | Distro | tkrzw package | defect 1 (`remove`) | defect 2 (comparator) | RESULT |
| --- | --- | --- | --- | --- | --- |
| `.deb` | Ubuntu 26.04 LTS | `1.0.32-1build1` | **broken** | **broken** (type 255, 0 relocs) | **broken** |
| `.deb` | Debian testing (forky) | `1.0.32-1+b2` | ok | ok (type 1, 18 relocs) | healthy |
| `.rpm` | Fedora Rawhide | `1.0.32-5.fc45` | ok | ok (type 1, 18 relocs) | healthy |
| `.rpm` | RHEL 10.2 + EPEL | `1.0.32-2.el10_1` | ok | ok (type 1, 18 relocs) | healthy |
| Arch | Arch, from source | none packaged | **broken** | ok (type 1, 18 relocs) | **broken (defect 1)** |

- **Ubuntu 26.04 LTS** (`1.0.32-1build1`, live from the archive)
  reproduces both defects, as the release row above predicts. `remove`
  stores the sentinel. The `.tkt` comparator records type 255 and the
  file does not reopen. The library keeps zero comparator GOT
  relocations.
- **Debian testing** (`1.0.32-1+b2`) is healthy. This matches the flags
  prediction and the clean result that the Launchpad reporter saw on
  trixie. `src:tkrzw` is orphaned in Debian (maintained by the Debian QA
  Group). The fix route under "Reported" meets the same status.
- **Fedora Rawhide** (`1.0.32-5.fc45`) is healthy, and is the most
  informative row. Fedora enables LTO by default, so its tkrzw would
  carry defect 1 like any LTO build. But the package deliberately avoids
  it: `tkrzw.spec` sets `%global _lto_cflags %{nil}`, with the changelog
  reason *"Disabled LTO, since it causes test failures on all file-based
  database tests."* An independent maintainer found defect 1 through the
  package's own `make check` and turned LTO off to get a working build.
  This independently confirms the bisection above. Fedora adds no
  `-Bsymbolic-functions`, so defect 2 does not occur.
- **RHEL 10.2** carries no tkrzw in its *own* repositories. `dnf install
  tkrzw` on `redhat/ubi10:latest` returns *"No match for argument:
  tkrzw"* across BaseOS, AppStream and CodeReady Builder. **EPEL 10 ships
  it**, as `tkrzw-1.0.32-2.el10_1` (vendor Fedora Project), and that
  build is **healthy**: 18 comparator relocations, `remove` clean,
  comparator type 1. EPEL follows the Fedora spec lineage, so it inherits
  the `%_lto_cflags %{nil}` opt-out. RHEL adds no `-Bsymbolic-functions`.
  So neither defect occurs. A tkrzw backend built against EPEL's libtkrzw
  on the RHEL 10.2 drop-in target is therefore safe. This was measured on
  that target (`crb`/EPEL enabled).
- **Arch** ships no tkrzw in `core`/`extra` (`pacman -Ss tkrzw` is
  empty). The only Arch source is the AUR `tkrzw-git`, a stale 2020 VCS
  stub (0 votes, building upstream HEAD with a plain
  `./configure && make`). With no package to probe, tkrzw 1.0.32 was
  built from the upstream release under Arch's stock `makepkg.conf`
  (gcc 16.2.1; `OPTIONS=(... lto)`, no `-Bsymbolic-functions`). It
  reproduces row C of the bisection exactly: `remove` broken, comparator
  intact. Arch has no distro-wide `_lto_cflags` opt-out. A tkrzw built
  there is therefore exposed to defect 1 unless its own PKGBUILD disables
  LTO. Neither the AUR stub nor a plain build does.

The two `.deb` rows, Fedora and RHEL-via-EPEL were installed directly
from each archive. Arch is the one row with no package. Its defect was
measured from a source build under the distro's own flags.

`distro-probe.sh` covers both defects. It was checked against all four
rows of the bisection: it reports A broken on both, B broken on the
comparator only, C broken on `remove` only, and D healthy. A distro that
prints `RESULT : healthy` has neither defect.

## tkrzw 1.0.34 fixes defect 1 upstream

This section records two measurements: 2026-10-01 (UTC) at `c99ffb57`,
and 2026-10-05 at `e985b581`, after #621 (`eaeb26fd`) moved the libpinyin
session into memory. The second measurement changes what a defect-1
library does to this backend, so the two stay separate below. The
drivers and probes that these measurements used are not in the tree. The
figures are recorded evidence.

Upstream changed how the sentinels are stored on 2026-09-20:
`estraier/tkrzw` commit `9ee8416`, "Change the object format of
DBM::RecordProcessor::NOOP and DBM::RecordProcessor::REMOVE" (ChangeLog:
"Fix bugs of address instability of NOOP and REMOVE"; release 1.0.34,
library 1.77.0). It addresses the sentinel half of "Upstream" above by a
third route. For defect 1 it is complete: an LTO build of 1.0.34 is
healthy. It does not change defect 2.

A report on 2026-10-01 named `9ee8416` as a regression. That run of the
tkrzw cell, against a from-source `9ee8416`, showed:

- `pinyin_save` returning `false` after any import or train, on both C
  ABIs;
- the pin-built libpinyin saving correctly against the same library;
- a rebuild at the 1.0.32 state (`bcaa0fb`) passing.

**That attribution does not hold, and the failure did not reproduce.**
oxpinyin passes against every build of `9ee8416` measured below, on two
architectures. The symptom itself is real and exact. It is what a
*defect-1* library did to this backend at that commit, and `9ee8416`
removes that fault. The reported run's environment was not examined, so
this record does not establish which libtkrzw it loaded. The inversion
stays recorded as unexplained.

### What the commit changes, and what it does not

```cpp
// tkrzw_dbm.cc at bcaa0fb (1.0.32)
const std::string_view DBM::RecordProcessor::NOOP("\x00\xBE\xEF\x02\x11", 5);
const std::string_view DBM::RecordProcessor::REMOVE("\x00\xDE\xAD\x02\x11", 5);

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

The type, the five bytes and the size are the same. Only the target of
each `string_view` changes. An anonymous string literal, which GCC may
materialise once per LTO partition, becomes a named array: one object
with one address. `DBM::ANY_DATA` gets the same change.

Nothing a client is compiled against moves:

- `tkrzw_langc.h`, `tkrzw_langc.cc` and `tkrzw_dbm.h` are the same git
  blobs at both commits (`7f25a305`, `fc8e56ae`, `61d13d2d`), and the two
  installed include trees are byte-identical.
- The C API's sentinels were never the C++ ones.
  `TKRZW_REC_PROC_NOOP` and `TKRZW_REC_PROC_REMOVE` are `(char*)-1` and
  `(char*)-2` (`tkrzw_langc.cc:60,62`): tags, not the address of any
  object. Both built libraries read back `0xffffffffffffffff` and
  `0xfffffffffffffffe`.
- The callback contract is the same. A `tkrzw_record_processor`
  (`tkrzw_langc.h:144-153`) returns one of the two tags, or a pointer
  whose length it stores through its last argument.
  `RecordProcessorWrapper` (`tkrzw_langc.cc:64-91`) compares that return
  against the tags by value, then substitutes the C++ sentinel.

So `oxpinyin-store`'s tkrzw backend needs no change to work with both
versions, and no way to tell them apart. It reads the two tags from the
library's own globals and returns them. Everything after that happens
inside libtkrzw, on either version. A version gate that refused 1.0.34
would refuse the one release that is immune to defect 1.

`bcaa0fb..9ee8416` holds two more commits, and neither is involved:

- `9db46db` (the 1.0.33 change) frees a leaked iterator key buffer in
  `TreeDBM` and `BabyDBM`. Only keys longer than 128 bytes reach it.
- `17926f7` is the version number and a regenerated `configure` that
  emits the same compiler and linker flags.

The upstream source was read from a fresh clone of
`https://github.com/estraier/tkrzw` at those commits. The blob ids are
`git rev-parse <commit>:<file>`.

### Measured 2026-10-01, at `c99ffb57`

At `c99ffb57`, a libpinyin session ran on a file-backed `TreeDBM`
scratch. The store suite counted 39 unit tests, with this section's test
in its first, three-class form.

Each commit was built with `./configure --prefix=… && make && make
install` in `debian:testing` (g++ 16.2.0), under three flag sets:

- **plain**: tkrzw's defaults, `-g -O2`;
- **LTO**: `-flto=auto -ffat-lto-objects`, LTO alone, as on Arch;
- **Ubuntu**: LTO plus `-Wl,-Bsymbolic-functions`.

arm64:

| tkrzw | flags | `.rodata` copies, NOOP / REMOVE | probe (c) (d) (e) | probe (b) | store suite | `pinyin_save`, `zhuyin_save` | the pin's save |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1.0.32 `bcaa0fb` | plain | 1 / 1 | ok | ok | 39 + 6 pass | `true` | `true` |
| `9db46db` | plain | 1 / 1 | ok | ok | 39 + 6 pass | `true` | `true` |
| 1.0.33 `17926f7` | plain | 1 / 1 | ok | ok | 39 + 6 pass | `true` | `true` |
| 1.0.34 `9ee8416` | plain | 1 / 1 | ok | ok | 39 + 6 pass | `true` | `true` |
| 1.0.32 `bcaa0fb` | LTO | 9 / 10 | **broken** | ok | **5 of 39 fail** | **`false`** | `true` |
| 1.0.34 `9ee8416` | LTO | 1 / 1 | ok | ok | 39 + 6 pass | `true` | `true` |
| 1.0.32 `bcaa0fb` | Ubuntu | 9 / 10 | **broken** | **broken** | **5 of 39 fail** | **`false`** | `true` |
| 1.0.34 `9ee8416` | Ubuntu | 1 / 1 | ok | **broken** | 39 + 6 pass | `true` | `true` |

The columns mean:

- "probe" is the identity probe above.
- The store suite is `oxpinyin-store`'s unit tests plus
  `tests/trait_laws.rs`.
- The two saves are the `save:` line of the `import-diff` and
  `zhuyin-import-diff` C drivers. They ran against oxpinyin's C ABI
  objects and against the pin-built libpinyin and libzhuyin, each loading
  the build in that row.

Where both saves read `true`, the two logs are identical, and so are the
two `nbest-train-diff` logs (the train path). Where oxpinyin reads
`false`, `nbest-train-diff` stops at `reopen: pinyin_save failed` and the
pin's run completes. `distro-probe.sh` agrees with the probe columns on
all eight builds. `9db46db` reports itself as 1.0.32, because its
checked-in `configure` was regenerated only at `17926f7`. Debian testing's
own package, `1.0.32-1+b2`, passes the same 39 + 6. CI's tkrzw child
links that library.

On the two plain builds the wider gates also pass:

- the tkrzw child of `store-backends.yml` (101 suites, 1076 passed,
  0 failed, 16 ignored; the same counts on `bcaa0fb` and `9ee8416`);
- the user-directory round trip end to end (Phase C IDENTICAL, Phase D
  READABLE, its system-import phase IDENTICAL for pinyin libraries 1–4);
- the zhuyin system-import round trip (IDENTICAL, libraries 1–4).

The next table has the same four cells on amd64 and the plain pair on
macOS. amd64 used the same image under emulation. The pin prefix is
arm64, so amd64 covers oxpinyin's side only. macOS is arm64, Apple clang
21.0.0, store suite only:

| tkrzw | flags | host | `.rodata` copies | probe (c) (d) (e) | store suite | `pinyin_save`, `zhuyin_save` |
| --- | --- | --- | --- | --- | --- | --- |
| 1.0.32 `bcaa0fb` | plain | amd64 | 1 / 1 | ok | 39 + 6 pass | `true` |
| 1.0.32 `bcaa0fb` | LTO | amd64 | 9 / 12 | **broken** | **5 of 39 fail** | **`false`** |
| 1.0.34 `9ee8416` | plain | amd64 | 1 / 1 | ok | 39 + 6 pass | `true` |
| 1.0.34 `9ee8416` | LTO | amd64 | 1 / 1 | ok | 39 + 6 pass | `true` |
| 1.0.32 `bcaa0fb` | plain | macOS | – | – | 39 + 6 pass | – |
| 1.0.34 `9ee8416` | plain | macOS | – | – | 39 + 6 pass | – |

Three results follow.

**Defect 1 is fixed at the mechanism, not masked.** The LTO build of
1.0.32 carries nine and ten copies of the two literals in `.rodata` (nine
and twelve on amd64). The LTO build of 1.0.34 carries one of each, on
both architectures.

**Defect 2 is unchanged.** With Ubuntu's flags, 1.0.34 still keeps no GOT
relocation for the comparators and still fails probe row (b). That does
not reach this backend, which installs no comparator. Its whole suite
passes on that build, and its logs match the pin's. On that evidence, a
1.0.34 built with Ubuntu's two flags is sound for oxpinyin and for
libpinyin. It is still unsound for `tkrzw_dbm_util` and any client that
names a comparator. Probe Ubuntu's own package, when it has one; do not
assume it is sound.

**Count the literal within one section.** An unstripped 1.0.34 holds
each five-byte pattern twice in the file: once in `.rodata`, and once in
`.debug_info` as the named array's constant value. The whole-file count
under "Defect 1" therefore reads 2 on a healthy build. Count within
`.rodata`.

### What a defect-1 library did at `c99ffb57`

The important row is 1.0.32 with LTO, because it matches the reported
symptom line for line:

- `pinyin_save` and `zhuyin_save` returned `false` after an import or a
  train, and nothing was logged. In the `import-diff` log the `save:`
  line was the only line that differed from a healthy run.
- The pin-built libpinyin, loading the same library, returned `true`.
  Its tkrzw sources never call `Rebuild` (`src/storage/*tkrzw*` at
  `074a2219`), so nothing on its save path can fail this way. Instead,
  the fault would land silently at its three `Remove()` sites, as
  described above. This is read from the source, not measured in the pin.
- The error that oxpinyin swallowed was `tkrzw status 6`
  (`TKRZW_STATUS_CANCELED_ERROR`, empty message) from
  `tkrzw_dbm_rebuild`. The path was `TkrzwStore::compact`, which
  `UserStore::save` called on the session's scratch file after it wrote
  the profile. `ContextCore::save_user` turns any `Err` into `false`.
  `false` is also the ABI's answer for "nothing modified" and "no user
  dir", so the three cannot be told apart from outside.
- The rebuild cancels because it replays every record through a
  processor that answers `NOOP` (`tkrzw_dbm_hash.cc:1914-1934`). It takes
  any other answer as a cancellation (`tkrzw_dbm_hash_impl.cc:427-428`,
  the only place where the DBM classes produce that status). This is the
  same `data()` comparison, between two partitions' copies of the
  literal. `TreeDBM` rebuilds through its `HashDBM`
  (`tkrzw_dbm_tree.cc:907`), so both file classes fail in the same way.
- The profile was already on disk. After `save: false`, the user dir held
  the same eleven files, at the same sizes, as after a successful save.
  The `false` came from the session scratch store failing to compact, not
  from the write.
- Removal is also wrong, and silent. Removing an absent record stores one
  whose value is `NOOP`'s five bytes (`00 be ef 02 11`). Removing a
  stored record leaves it in place with `REMOVE`'s bytes
  (`00 de ad 02 11`). Both writes report success. That held on all three
  DBM classes that the backend then opened (`TreeDBM`, `HashDBM`, the
  user bigram's `TinyDBM`).

In the store suite the same faults read:

```
tests::tkrzw_write::remove_in_write                 left: Some([0, 190, 239, 2, 17])  right: None
user_bigram_db_tests::store_get_remove_and_the_walk_see_the_same_records
                                                    left: Some([0, 222, 173, 2, 17])  right: None
tests::tkrzw_write::compact_preserves_data          Err(Backend(TkrzwError { code: 6, message: "" }))
tests::tkrzw_write::user_bigram_round_trips_through_its_own_seam
                                                    Err(Backend(TkrzwError { code: 6, message: "" }))
```

`tests::tkrzw_library_honours_its_sentinel_protocol` reports the fault
in words and names this document. In its first form it passed on the six
healthy builds above and failed on the two defect-1 builds.

The C API does not protect the backend from any of this. "It is not only
the CLI" gives the reason: the sentinel fault is inside libtkrzw, between
the wrapper that returns the C++ sentinel and the DBM class that compares
it. Reading the tags from the library's globals guarantees agreement at
the boundary and nothing beyond it.

### What #621 changed — re-measured 2026-10-05, at `e985b581`

#621 (`eaeb26fd`) moved the libpinyin session off its temp file. The
session runs on `WriteStore::create_in_memory`. On this backend that is
tkrzw's `BabyDBM` through PolyDBM's empty-path form: a fourth DBM class
beside `TreeDBM`, `HashDBM` and the user bigram's `TinyDBM`.

**The save path still reaches `tkrzw_dbm_rebuild`, but the call cannot
fail.**

- Every shipping save (`pinyin_save`, `zhuyin_save`) goes through
  `ContextCore::save_user` (`crates/oxpinyin-facade/src/context.rs:284-287`)
  into `GenericUserStore::save`. That function still ends by compacting
  the session store (`crates/oxpinyin-user/src/store.rs:1516`), and
  `TkrzwStore::compact` still calls `tkrzw_dbm_rebuild`.
- On a `BabyDBM` that call is a no-op. `PolyDBM::RebuildAdvanced` falls
  through to `BabyDBM::Rebuild` (`tkrzw_dbm_poly.cc:755`). Its whole body
  is `return Status(Status::SUCCESS)` under the comment "This method does
  nothing" (`tkrzw_dbm_baby.h:284-290`, blob `0b934760` at both commits).
- The only other `compact` call outside tests, benches and examples is the
  default `WriteStore::write_user_bigram`
  (`crates/oxpinyin-store/src/lib.rs:419`), on a `HashDBM` file. No
  shipping path reaches it. `persistence::save_with_bigram` takes that
  branch only without a bigram container, and a libpinyin session always
  passes its own. `persistence::save`, which passes none, has test
  callers only.
- A store opened on a file path (`UserStore::open`, `create_standalone`)
  would still compact a `TreeDBM`. Only tests, benches, examples and the
  fuzz target open one.

So a shipping save cannot reach the record replay that cancels, which is
the mechanism behind every `false` in the table above.

**The defect is still reachable, through removal.** The table uses the
same drivers at `e985b581`, with each side loading the build in its row.
It adds an `abi-probe-diff` driver (sort word `1e`), which saves after an
import and again after a `pinyin_mask_out`:

| tkrzw | flags | store suite | `import-diff`, `zhuyin-import-diff` save | `nbest-train-diff` | `abi-probe-diff` save after import, after `mask_out` | the pin, same four |
| --- | --- | --- | --- | --- | --- | --- |
| Debian `1.0.32-1+b2` | package | 40 + 6 pass | – | – | – | – |
| 1.0.32 `bcaa0fb` | LTO | **6 of 40 fail** | `true` | completes | `true`, **`false`** | `true`, completes, `true`, `true` |
| 1.0.34 `9ee8416` | LTO | 40 + 6 pass | `true` | completes | `true`, `true` | `true`, completes, `true`, `true` |

On the LTO 1.0.32, the first three drivers' logs are identical to the
pin's; the `false` of 2026-10-01 does not occur. The fourth log differs in
three observations:

- `save(after-mask)` is `false`, against the pin's `true`.
- In the context that it then opens on the same user dir,
  `lookup_tokens(你好)` gives two tokens against one.
- The token that `phrase_token[0]` reports differs.

The last two differ because the mask that could not be saved is not in
the profile. On the LTO 1.0.34 all four logs are identical.

This is the removal fault of the list above, now inside the session.
`GenericUserStore::mask_out` issues `WriteTxn::remove` on the session
store. `remove_user_phrase` does the same behind
`pinyin_remove_user_candidate`, and so does a train that empties a
predecessor's row on the user bigram's `TinyDBM`. On a defect-1 library,
`pinyin_mask_out` returns `true`, but the rows that it removed stay in the
`BabyDBM`, with `REMOVE`'s five bytes. The next save that has something
to write runs `export_state` (the probe's context was already modified).
`export_state` reads a pronunciation row that should be gone. It finds
five bytes where a value is 8 or 13, and fails with
`corrupt pronunciation value`. `ContextCore::save_user` turns that into
`false`. The pin, on the same library, answers `true`.

So since #621, a defect-1 library is quieter on this backend, not safer.
A save after an import or a train succeeds and matches the pin. The first
removal breaks the session, and no call reports it until the next save
with something to write. #677 tracks this and records the sequence save
by save.

In the store suite, the in-memory class adds a sixth failure on the LTO
1.0.32: `tests::tkrzw_write::in_memory_store_behaves_as_a_tree_store`
(`left: Some([0, 222, 173, 2, 17])`). This section's test checks four
classes. On that library, `BabyDBM` stores a record for an absent removal
and keeps a removed one, as the other three classes do. Its `compact`
succeeds.

### The removal behind `save(after-mask)`, call by call

This section gives three facts for the work on that failing save. They
are the first two things that #677 asks to establish before a fix.
Nothing here is implemented. How the store removes a record is shipped
code, and a change to it is that issue's work.

**The oxpinyin call that relies on the sentinel.** The rows that outlive
the mask are the masked phrases' pronunciation rows, which
`GenericUserStore::mask_out` removes with `txn.remove(PRONUNCIATION, …)`
at `crates/oxpinyin-user/src/store.rs:1724`, after the bigram, unigram
and phrase rows at `:1643-1718`. The tkrzw backend only buffers that
removal (`TkrzwWriteTxn::remove`,
`crates/oxpinyin-store/src/tkrzw/mod.rs:1012-1016`). The call that relies
on the sentinel comes at commit. `db_apply` hands the batch to
`tkrzw_dbm_process_multi` (`mod.rs:741`). Its callback `apply_one`
answers `TKRZW_REC_PROC_REMOVE` for a record that exists (`mod.rs:569`)
and `TKRZW_REC_PROC_NOOP` for one that does not (`:567`).
libtkrzw's wrapper turns the tag into the C++ `REMOVE`, and `BabyDBM`
compares that by address (`tkrzw_dbm_baby.cc:1081-1103`). The user
bigram's `TinyDBM` takes the same route (`TkrzwUserBigramDb::remove`,
`mod.rs:1141`, through `db_set_one`, `:1078`). The next save reads the
surviving row back at `crates/oxpinyin-user/src/store_libpinyin.rs:441`.
All at `e985b581`.

**How the pin removes the same record.** It does not ask tkrzw to,
because the record is not in tkrzw. A phrase's pronunciations live in its
`PhraseItem` in the in-memory phrase index, and `pinyin_mask_out`
(`src/pinyin.cpp:1224-1296` at `074a2219`) drops the item there
(`:1290`, into `SubPhraseIndex::mask_out`,
`src/storage/phrase_index.cpp:677-697`). The two tkrzw index tables it
masks are the user ones, in-memory `BabyDBM`s
(`chewing_large_table2_tkrzwdb.cpp:98`,
`phrase_large_table3_tkrzwdb.cpp:89`), and it rewrites them in place: a
writable `ProcessEach` whose processor returns each record's masked
value (`chewing_large_table2_tkrzwdb.cpp:419-491`,
`phrase_large_table3_tkrzwdb.cpp:277-315`). Taking one entry out of an
index record is likewise a `Set` of the shorter record (`:377` and
`:271` of the same two files), never a removal. The one container the
pin deletes records from is the user bigram, a `TinyDBM`
(`ngram_tkrzwdb.cpp:52`): `Bigram::mask_out` (`:175-212`) calls
`Bigram::remove`, which is tkrzw's plain `m_db->Remove(key)` (`:140`).

**Whether tkrzw's plain remove avoids the sentinel path.** It does not,
on any class that the store uses. `tkrzw_dbm_remove` is `PolyDBM::Remove`
(`tkrzw_langc.cc:791-803`, `tkrzw_dbm_poly.cc:612-617`), and
`DBM::Remove` is itself a record processor: `RecordProcessorRemove`
answers the C++ `REMOVE` for a stored record and `NOOP` for an absent
one (`tkrzw_dbm.h:217-242`) through the same `Process`
(`tkrzw_dbm.h:1196-1204`; `HashDBM`'s override differs by one flag,
`tkrzw_dbm_hash.cc:2448-2456`). It skips the C wrapper's two tags and
nothing after them. `tkrzw_dbm_remove` on the LTO 1.0.32, two records
stored:

| class | removing a stored key | removing an absent key | records left |
| --- | --- | --- | --- |
| `TreeDBM`, `HashDBM`, `BabyDBM`, `TinyDBM` | returns `true`; the record stays, value `00 de ad 02 11` | returns `false`, `NOT_FOUND`; a record appears, value `00 be ef 02 11` | 3 |

On the LTO 1.0.34 and on Debian's 1.0.32 package the same calls return
`true` and `false`, both keys read back absent, and one record is left,
on all four classes.

The pin's own exposure is that one call. Its user-bigram `Remove`
(`ngram_tkrzwdb.cpp:140`) is `DBM::Remove` on a `TinyDBM` and goes
through the same sentinel. It is one of the three `Remove()` sites under
"Does anything actually ship against this?". **That is inferred, not
measured.** It rests on the source and on the probe above, whose
`TinyDBM` case reaches the same `DBM::Remove` through the C API. The pin
was not driven through `Bigram::remove` on the LTO 1.0.32. The
`abi-probe-diff` run observed only that its `pinyin_mask_out` and both of
its saves answered `true`, and nothing below the ABI. This record
therefore does not establish whether that mask-out reached the call, or
what the pin's user bigram held afterwards. The pin's mask-out differs
from oxpinyin's in where the masked records live, not in how tkrzw
removes them. Its phrase items are not tkrzw records, and it rewrites its
index records; it does not remove them.

### If `pinyin_save` returns `false` on tkrzw

On a defect-1 library, no save fails until the context removes
something (`pinyin_mask_out` is the measured case). Then the next save
with something to write fails, while the pin still saves `true` (#677).
Check the build of the library, not its version. Below 1.0.34 the version
tells you nothing.

1. **Find the libtkrzw that the failing process loaded.** It need not be
   the one that the build linked. `oxpinyin-store`'s build script puts a
   RUNPATH only on that package's own targets: its unit-test,
   `trait_laws` and example binaries (by `readelf -d`). `rustc-link-arg`
   is package-scoped. `libpinyin_capi.so`, `libzhuyin_capi.so` and the
   other crates' test binaries carry no RUNPATH. They load the
   `libtkrzw.so.1` that `LD_LIBRARY_PATH` or the loader cache names. So a
   store suite that passes and a C-ABI driver that fails in the same
   shell can use two different libraries. Use `ldd`, or `LD_DEBUG=libs`
   on the failing command, to find out.
2. **Probe that library.** Run `tools/tkrzw/distro-probe.sh` against it,
   or run this backend's own check:

   ```sh
   PKG_CONFIG_PATH=$P/lib/pkgconfig LD_LIBRARY_PATH=$P/lib \
     cargo test -p oxpinyin-store tkrzw_library_honours_its_sentinel_protocol
   ```
3. **Then read the version.** 1.0.34 passed under every flag set
   measured here. Whether 1.0.33 and earlier are healthy depends on the
   build. They are healthy from `./configure && make` and from Debian,
   Fedora and EPEL. They are broken from Ubuntu and under Arch's default
   flags.

CI never links a distro libtkrzw. Every job uses the tkrzw action's build
of `TKRZW_REV` (`.github/actions/tkrzw`).

### Provenance — the command behind each figure

2026-10-01 (UTC), on an arm64 macOS host in Docker: `debian:testing` at
`debian@sha256:16faa8d1cd99fcb2d30eebe90454e26b20499055fa7094e9b55d32d1a7666f08`
(native, and `--platform linux/amd64` for the amd64 rows), g++ 16.2.0
(Debian 16.2.0-3), GNU ld 2.47, rustc 1.97.1. The apt set was the
`store-backends.yml` test job's set without `libtkrzw-dev`,
`libkyotocabinet-dev` and `libdb-dev`, plus `python3` and `gdb`. So no
distro libtkrzw existed in either container, and a binary that missed the
build under test could not fall back to another. oxpinyin was at
`c99ffb57` plus this section's change in its first form.

2026-10-05 (UTC), the same host under Apple `container` 1.5.0: the same
image digest, arm64 native, the same g++, ld and rustc. The apt set was
that test job's full set, plus `python3` and `gdb`. Debian's
`libtkrzw-dev 1.0.32-1+b2` was therefore present beside the two
from-source LTO builds. Every run was checked for the libtkrzw that it
loaded (the row below). oxpinyin was at `e985b581` plus this change.

The C drivers (`import-diff`, `zhuyin-import-diff`, `nbest-train-diff`,
`abi-probe-diff`), the identity probe and the round-trip scripts are not
in the tree. Their figures are recorded evidence. The table keeps the
commands that need no deleted tool.

`$P` is one build's prefix. `$T` is a separate `CARGO_TARGET_DIR` per
build. `$ORACLE` is a prefix from `tools/oracle/build.py`
(libpinyin 2.11.92 at `074a2219`, `--with-dbm=Tkrzw`, with libzhuyin). The
oracle was built once, on 2026-09-25 against Debian testing's 1.0.32
package, and not rebuilt per row. It reaches each build through the
loader, which is sound because the headers are identical.

| figure | command |
| --- | --- |
| sources | `git clone https://github.com/estraier/tkrzw`, then `git archive <commit>` into one tree per build |
| the commits in the range, their dates | `git log --format='%h %cs %s' bcaa0fb..9ee8416` |
| what each commit changes | `git show --stat <commit>`; the sentinel change is `git diff bcaa0fb 9ee8416 -- tkrzw_dbm.cc`; the leak fix is `git show 9db46db`, its bound `sizeof(stack_)` with `ITER_BUFFER_SIZE = 128` (`tkrzw_dbm_tree.cc:55`, `tkrzw_dbm_baby.cc:35`) |
| release and library numbers | `git show 9ee8416:ChangeLog \| head -6` |
| cited tkrzw lines | `git show <commit>:<file> \| sed -n '<first>,<last>p'`; apart from the two `tkrzw_dbm.cc` quotations, every range cited in this section reads the same at `bcaa0fb` and at `9ee8416` |
| blob ids | `git rev-parse <commit>:<file>` |
| build, plain | `./configure --prefix=$P && make && make install` |
| build, LTO | the same with `CXXFLAGS="-g -O2 -flto=auto -ffat-lto-objects"`, the same `CFLAGS`, `LDFLAGS="-flto=auto -ffat-lto-objects"` |
| build, Ubuntu | as LTO, with `LDFLAGS="-Wl,-Bsymbolic-functions -flto=auto -ffat-lto-objects -Wl,-z,relro"` |
| same flags across the range | `grep -m1 'tkrzw_dbm.cc' make.log` and `grep -m1 -e '-shared' make.log` on each build's `make` output; across the four commits the two lines differ only in the prefix, the two version macros and the library's file name |
| the version a build reports | the `tkrzw  :` line of `distro-probe.sh`, which is `$P/bin/tkrzw_build_util version`; `PKG_CONFIG_PATH=$P/lib/pkgconfig pkg-config --modversion tkrzw` agrees |
| include trees | `diff -r $P_bcaa0fb/include $P_9ee8416/include` |
| tag values | `gdb -batch -ex 'p/x (long)TKRZW_REC_PROC_NOOP' -ex 'p/x (long)TKRZW_REC_PROC_REMOVE' $P/lib/libtkrzw.so.1` |
| `.rodata` copies | `objcopy --dump-section .rodata=ro.bin $P/lib/libtkrzw.so.1 discard.so && LC_ALL=C grep -a -o -P '\x00\xBE\xEF\x02\x11' ro.bin \| wc -l` for `NOOP`, `'\x00\xDE\xAD\x02\x11'` for `REMOVE`; `.debug_info` the same way, and the `grep` on the library itself for the whole-file count. The tables' counts were first read by a script that placed each whole-file match in a section by the offsets of `readelf -SW`; this command gave the same counts on the two LTO builds when re-run on 2026-10-05 |
| comparator relocations | `readelf -rW $P/lib/libtkrzw.so.1 \| grep -c KeyComparator` |
| `distro-probe.sh` | `PATH=$P/bin:$PATH LD_LIBRARY_PATH=$P/lib sh tools/tkrzw/distro-probe.sh` |
| store suite | `PKG_CONFIG_PATH=$P/lib/pkgconfig LD_LIBRARY_PATH=$P/lib CARGO_TARGET_DIR=$T cargo test --locked --no-fail-fast -p oxpinyin-store --no-default-features --features tkrzw` |
| store suite, Debian package | the `store-backends.yml` test job's apt line in a fresh `debian:testing`, then `RUSTFLAGS="-D warnings" cargo test --locked --no-fail-fast -p oxpinyin-store --no-default-features --features tkrzw`; the package is `dpkg-query -W libtkrzw-dev libtkrzw1t64` |
| store suite, macOS | `env -u PKG_CONFIG_PATH PKG_CONFIG_LIBDIR=/nonexistent OXPINYIN_TKRZW_INCLUDE_DIR=$P/include OXPINYIN_TKRZW_LIB_DIR=$P/lib OXPINYIN_TKRZW_LIB_NAME=tkrzw CARGO_TARGET_DIR=$T cargo test …` as above; pkg-config is pointed away because a from-source `tkrzw.pc` names `-lstdc++`, which macOS does not have, on both commits |
| each class, each fault | the test stops at its first failed assertion — on the LTO 1.0.32, `TreeDBM`'s absent removal — so the other class-and-fault pairs were read from scratch copies of `crates/oxpinyin-store/src/lib.rs`, never committed: the class under study moved ahead of the others, the assertions before the one under study deleted, each copy run with the store-suite command and the test's name as its filter. Both dates; the 2026-10-05 copies add `BabyDBM` |
| C ABI objects | the same three variables, `cargo build --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi --no-default-features --features tkrzw` |
| what the pin calls | `git grep -nE 'Rebuild\|Remove\(\|Synchronize\|ProcessEach' 074a2219 -- 'src/storage/*tkrzw*'` |
| RUNPATH | `readelf -d <object> \| grep RUNPATH` on `oxpinyin-store`'s unit-test, `trait_laws` and example binaries, on the `oxpinyin-user`, `-data` and `-runtime` test binaries and on the two C ABI objects, all under `$T/debug` |
| environment | `g++ --version`, `ld --version` and `rustc --version` in the container; `sw_vers`, `clang --version` and `container --version` on the host; the image digest is `docker image inspect --format '{{index .RepoDigests 0}}' debian:testing` on 2026-10-01, and the 2026-10-05 container was started from that digest |
| 2026-10-05: store suite | the store-suite command at `e985b581`, without its first two variables for Debian's package |
| 2026-10-05: the library a run loaded | `ldd <test binary> \| grep tkrzw`; `LD_DEBUG=libs <driver> … 2>&1 \| grep 'calling init: .*libtkrzw'` |
| 2026-10-05: Homebrew | `LIBRARY_PATH=$(brew --prefix)/lib cargo test --locked --no-fail-fast -p oxpinyin-store --no-default-features --features tkrzw` on the macOS host (`brew list --versions tkrzw`: 1.0.32) |
| the hosted runner's bottle, 2026-10-01 | `gh run view 36832757541 --job 110272962477 --log \| grep 'Pouring tkrzw'` |
| 2026-10-05: `compact` callers | `git grep -nE '\.compact\(' e985b581 -- 'crates/*.rs'`, then the callers of `write_user_bigram`, `stage_user_bigram`, `persistence::save`, `UserStore::open` and `create_standalone` |
| 2026-10-05: tkrzw's plain remove, per class | the scratch program printed below the table, which is in no repository: `gcc -std=gnu11 -Wall -Wextra -O2 -I $P/include plain-remove-probe.c -o probe -L $P/lib -Wl,-rpath,$P/lib -ltkrzw`, then `./probe` in an empty directory; for Debian's package, the same without `-I`, `-L` and `-rpath` |
| 2026-10-05: the pin's mask-out | libpinyin at `074a2219` from a local mirror clone; blobs `f27f7cf7` (`src/pinyin.cpp`), `d58e37e2` (`phrase_index.cpp`), `8ae83e01` (`chewing_large_table2_tkrzwdb.cpp`), `0e4094fe` (`phrase_large_table3_tkrzwdb.cpp`), `c826a444` (`ngram_tkrzwdb.cpp`) by `git rev-parse 074a2219:<file>` |
| 2026-10-05: CI's libtkrzw per job | every `container:`, `runs-on:`, `libtkrzw`, `brew install` and `--features` line of `.github/workflows/*.yml` at `e985b581`, and the `cargo` lines of the scripts they call |

One row rests on a scratch source that is in no repository, so it is
printed here in full.

tkrzw's plain remove, per class: `plain-remove-probe.c`. This is a
shortened form of the program that produced the per-class table. It was
run again on 2026-10-05 against the same three libraries, with the same
results. On a healthy library every class prints `remove(stored)=1
remove(absent)=0/status 7 removed=absent absent=absent count=1`; on the
LTO 1.0.32 every class prints `removed=00dead0211 absent=00beef0211
count=3` after the same two answers:

```c
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <tkrzw_langc.h>

static void show(TkrzwDBM *dbm, const char *key) {
    int32_t size = 0;
    char *value = tkrzw_dbm_get(dbm, key, -1, &size);
    printf(" %s=", key);
    if (value == NULL) {
        printf("absent");
    } else {
        for (int32_t i = 0; i < size; i++) printf("%02x", (unsigned char)value[i]);
        free(value);
    }
}

static void probe(const char *name, const char *path, const char *params) {
    TkrzwDBM *dbm = tkrzw_dbm_open(path, true, params);
    tkrzw_dbm_set(dbm, "kept", -1, "v", -1, true);
    tkrzw_dbm_set(dbm, "removed", -1, "v", -1, true);
    bool stored = tkrzw_dbm_remove(dbm, "removed", -1);
    bool absent = tkrzw_dbm_remove(dbm, "absent", -1);
    int32_t code = tkrzw_get_last_status().code;
    printf("%s remove(stored)=%d remove(absent)=%d/status %d", name, stored, absent, code);
    show(dbm, "removed");
    show(dbm, "absent");
    printf(" count=%lld\n", (long long)tkrzw_dbm_count(dbm));
    tkrzw_dbm_close(dbm);
}

int main(void) {
    probe("TreeDBM", "plain-remove.tkt", "dbm=tree,truncate=true");
    probe("HashDBM", "plain-remove.tkh", "dbm=hash,truncate=true");
    probe("BabyDBM", "", "dbm=baby");
    probe("TinyDBM", "", "dbm=tiny");
    return 0;
}
```

### Evidence

This document commits no captures: no build, probe, test or driver log of
either date is committed. Figures from the deleted drivers are recorded
evidence. The other figures rest on the commands in the table. Where a
command needs a source that is in no repository, that source is printed
in full below the table.
