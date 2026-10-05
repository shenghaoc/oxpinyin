# Debian now ships libpinyin on tkrzw; Ubuntu's tkrzw silently corrupts records

Date: 2026-08-28 (cross-distro matrix measured 2026-08-29) · Status:
**investigation finding** (no shipping code changed; guidance and two
probes added) · Branch: `claude/tkrzw-distro-compat-9o3k8h`.
**Amended 2026-10-05:** tkrzw 1.0.34 (`9ee8416`, 2026-09-20) fixes
defect 1 upstream and leaves defect 2, so everything below about
defect 1 describes 1.0.33 and earlier. The measurements — and what a
defect-1 library does to this backend, before and after its libpinyin
session moved into memory — are the last section, "tkrzw 1.0.34 fixes
defect 1 upstream" (branch `claude/tkrzw-134-sentinel-finding`; no
shipping code changed, one store test added).

`oxpinyin-store`'s tkrzw backend already carried a warning that Ubuntu
noble's `libtkrzw-dev 1.0.27-1.1build1` breaks tkrzw's pointer-identity
protocol, and that `./configure && make` on the same sources is correct.
This note establishes why, and how far it reaches — and the answer to
"how far" moved while it was being written.

**The stakes changed: Debian has switched `libpinyin` to the tkrzw
backend** (`2.11.91-1`, unstable/testing, 2026-08-12), so these faults
now sit under a shipped input method's user dictionary, not only a
command-line tool. Debian's tkrzw package is healthy (measured
`1.0.32-1+b2`), so that shipped combination is safe today; the exposure
is Ubuntu, whose tkrzw (`1.0.32-1build1`) is broken on both defects
below, and which syncs `libpinyin` from Debian. See "Does anything
actually ship against this?" for the measured per-distro backend matrix.

**There are two independent defects, with two different causes.** Both
come from build flags Ubuntu applies to every package and Debian applies
to none, both are silent, and neither fixes the other:

| | Cause | Breaks | Symptom |
| --- | --- | --- | --- |
| **1** | `-flto` | `RecordProcessor::NOOP` / `REMOVE` — *data* | `Remove()` stores a tombstone instead of deleting; a NOOP processor overwrites the record; `Rebuild` mis-counts |
| **2** | `-Wl,-Bsymbolic-functions` | the key comparators — *functions* | TreeDBM records comparator type 255 and can never reopen the file |

This matters for the fix: Ubuntu bug [LP #2142937][lp] carries a patch
that disables LTO, which resolves defect 1 and leaves defect 2 exactly
as it was. See "Reported" below.

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

Three consecutive releases, two upstream versions, three different
Debian revision strings, both defects in every one. The development
series still carries resolute's binary unchanged, so as of this note no
fix has landed anywhere in Ubuntu.

Questing's row is worth reading twice. Its version string is
`1.0.32-1` — *identical* to Debian's, with no `buildN` suffix, because
it is a straight source sync rather than a no-change rebuild. Same
source, same version string, different binary, because Ubuntu built it.
The `buildN` suffix is not what marks an affected package; being built
by Ubuntu is.

And the same source built elsewhere:

| Build | `remove` | `.tkt` round-trip |
| --- | --- | --- |
| upstream `1.0.27`, `./configure && make` | ok | ok |
| Debian source pkg `1.0.27-1.1`, Debian flags | ok | ok |
| Debian source pkg `1.0.27-1.1`, Ubuntu flags | **tombstone** | **unreadable** |
| Debian source pkg `1.0.32-1`, Debian flags | ok | ok |
| Debian source pkg `1.0.32-1`, Ubuntu flags | **tombstone** | **unreadable** |

`1.0.32-1` is the version that matters: it is what Debian uploaded to
unstable and what Ubuntu ships in questing, resolute and devel. Built
with Ubuntu's flags it reproduces the shipped failure exactly; built
with Debian's, from the same tarball on the same machine, it is clean.

"unreadable" is literal: `tkrzw_dbm_util create x.tkt` succeeds, and
every subsequent open of that file by the same binary fails with
`BROKEN_DATA_ERROR: invalid_key_comparator`. SkipDBM (`.tks`) is
unaffected by defect 2 — it stores no comparator — but every DBM type is
exposed to defect 1.

## The bisection

Four real rebuilds of Debian's `tkrzw_1.0.32-1` source package, all with
`DEB_VENDOR=Ubuntu`, varying only the two flags:

| Build | LTO | `-Bsymbolic-functions` | `remove` | `.tkt` | comparator GOT relocs |
| --- | --- | --- | --- | --- | --- |
| A — stock Ubuntu | on | on | **tombstone** | **unreadable** | 0 |
| B — `optimize=-lto` (the LP patch) | off | on | ok | **unreadable** | 0 |
| C — strip the linker flag only | on | off | **tombstone** | ok | 18 |
| D — strip both | off | off | ok | ok | 18 |
| Debian vendor, for reference | off | off | ok | ok | 18 |

The two columns are cleanly orthogonal. LTO alone governs `remove`;
`-Bsymbolic-functions` alone governs the comparator. Row D and the
Debian reference agree, which is the check that nothing else in
Ubuntu's flag set is involved.

An earlier revision of this note claimed `-Bsymbolic-functions` was
"necessary and sufficient" and called LTO a red herring. That was
measured only against defect 2, where it holds, and wrongly generalised
to defect 1. Row C above is the counter-example.

## Defect 2 — the comparators

tkrzw identifies some values by the *address* of a symbol rather than by
its contents, and says so: `tkrzw_dbm.h` documents that the
`RecordProcessor::NOOP` sentinel must be checked with
`your_value.data() == NOOP.data()`. `TreeDBM` does the same for
comparators — `tkrzw_dbm_tree.cc:1150` turns the caller's function
pointer into the on-disk type byte purely by pointer equality:

```cpp
if (key_comparator_ == nullptr || key_comparator_ == LexicalKeyComparator) {
  key_comp_type = 1;
} else if (key_comparator_ == LexicalCaseKeyComparator) {
  ...
} else {
  key_comp_type = 255;          // "a comparator I cannot name"
}
```

On reopen, `LoadMetadata` maps 1..5 and 101..105 back to a built-in;
byte 255 means "the caller must re-supply the same custom comparator",
and nothing in tkrzw can do that on its own — so the file is unreadable
(`tkrzw_dbm_tree.cc:1237`).

The five comparators are `inline` functions in a header
(`tkrzw_key_comparators.h:43`). Any translation unit that takes their
address emits its own COMDAT copy, so `tkrzw_dbm_util` has one and
`libtkrzw.so` has another. C++ requires the two to compare equal, and
ELF delivers that by routing the library's address-taking through the
GOT: a `R_X86_64_GLOB_DAT` relocation the dynamic linker resolves to the
one canonical definition. A correct build of 1.0.32 keeps 18 such
relocations; a broken one keeps none.

`-Wl,-Bsymbolic-functions` is exactly the flag that removes them. It
binds a shared library's function references to that library's own
definitions at link time. `libtkrzw.so` then compares against its own
copy, `tkrzw_dbm_util` passes its own copy — and no built-in comparator
is ever recognised. `--comparator` defaults to `lex`
(`tkrzw_dbm_util.cc:630`), so *every* `.tkt` the CLI creates records 255.

## Defect 1 — the sentinels

`NOOP` and `REMOVE` are not functions but static data, each a
`string_view` over a five-byte literal (`tkrzw_dbm.cc:25`):

```cpp
const std::string_view DBM::RecordProcessor::NOOP("\x00\xBE\xEF\x02\x11", 5);
const std::string_view DBM::RecordProcessor::REMOVE("\x00\xDE\xAD\x02\x11", 5);
```

`-Bsymbolic-functions` cannot touch these — it binds functions only —
and indeed row B above still passes `remove` while failing `.tkt`. What
breaks them is LTO, and the mechanism is visible in the binary. Counting
occurrences of each backing literal inside `libtkrzw.so`:

```
A stock Ubuntu (LTO + Bsym)    NOOP=10   REMOVE=11
B LP patch  (no LTO, Bsym)     NOOP=1    REMOVE=1
C LTO, no Bsym                 NOOP=10   REMOVE=11
D neither                      NOOP=1    REMOVE=1
```

Without LTO there is exactly one copy of each, so the exported `NOOP`
object and every comparison site necessarily agree. With LTO, GCC's
partitioning gives most partitions their own copy — ten and eleven of
them — so the address a comparison site was compiled against is not the
address the exported object carries, and
`value.data() == NOOP.data()` fails. `Remove` then writes the REMOVE
sentinel as the record's value and reports success.

## It is not only the CLI

Both defects reach any *client* of the library, which is what makes them
dangerous for a backend like ours — and defect 1 reaches further than
that, since row (d) below calls plain `dbm.Remove(key)` and passes no
sentinel across the boundary at all. A client compiled against each
build (`tools/tkrzw/identity-probe.cc`):

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

Row (a) is the one piece of good news: leaving `key_comparator` at its
`nullptr` default keeps the decision inside the library, where both
sides of the comparison are the same copy. `oxpinyin-store` installs no
comparator, so defect 2 does not reach its files. Rows (c)–(e) it *is*
exposed to, and they are silent — a `Remove` that stores the sentinel as
the value looks like a successful write.

## What decides it, per distro

Each defect reduces to one bit about the distro's defaults, and both
bits are readable from primary sources without installing tkrzw.

| Distro | Source | `-Bsymbolic-functions` | LTO by default |
| --- | --- | --- | --- |
| Ubuntu | `Dpkg::Vendor::Ubuntu:156` | **yes** | **yes** |
| Debian | `Dpkg::Vendor::Debian` | no | no (opt-in per package) |
| Devuan, PureOS | their `Dpkg::Vendor::*` | no | no |
| Arch | `/etc/makepkg.conf` (`pacman 7.1.0.r9`) | no | **yes** (`OPTIONS=(... lto)`) |
| Fedora | `redhat-rpm-config` | no | **yes** globally, but `tkrzw.spec` sets `%_lto_cflags %{nil}` |
| RHEL 10 (via EPEL) | EPEL, Fedora spec lineage | no | no (`tkrzw.spec` opts out); not in RHEL's own repos |

The dpkg figures are from dpkg's own git at tag **1.23.7** — the exact
dpkg the `debian:sid-slim` image reports — so this is Debian's file, not
Ubuntu's copy of it. Arch's full default `LDFLAGS` are

```
-Wl,-O1 -Wl,--sort-common -Wl,--as-needed -Wl,-z,relro -Wl,-z,now -Wl,-z,pack-relative-relocs
```

and its `-fno-plt` is not a hazard here: it changes how calls are
routed, not how addresses are taken.

**Debian is clean on both counts; Arch is clean on defect 2 but exposed
to defect 1** — Arch enables LTO globally via `OPTIONS=(... lto)`. Both
are now measured rather than predicted (see "The matrix, measured"
below): Debian testing probes healthy, and Arch — which packages no
tkrzw to probe — reproduces defect 1 and not defect 2 from a source
build under its own flags, exactly as the flag bits say. The Launchpad
reporter had already run Debian and got the same clean result on
`1.0.32-1+b1` from trixie.

A package can still reintroduce either flag itself, and tkrzw's does
not: `debian/rules` never sets `LDFLAGS` — its only two mentions are a
commented-out `DEB_LDFLAGS_MAINT_APPEND` — and the file is byte
identical from `1.0.27-1.1` through `1.0.32-1`.

## Does anything actually ship against this?

The question that decides how much any of it matters: has a distro
switched libpinyin from Berkeley DB to tkrzw, where these faults would
land on a user's dictionary rather than on a command-line tool?

**Yes — Debian has, as of 2026-08-12.** `libpinyin 2.11.91-1`, in
unstable and testing (sid, forky), sets `--with-dbm=Tkrzw` in
`debian/rules` and Build-Depends `libtkrzw-dev`; its shipped
`libpinyin.so.15` links `libtkrzw.so.1` and depends on
`libtkrzw1t64 (>= 1.0.32)` — measured, not inferred. The changelog is
explicit — "Switch from BerkeleyDB to Tkrzw" (Closes: #1119204,
#993415) — and `debian/NEWS` warns that after "the engine switch from
BerkeleyDB to Tkrzw ... all previous user data will be lost after the
upgrade." Upstream only added the Tkrzw option in 2.11.91; its
`configure.ac` still defaults to `DBM="BerkeleyDB"` and its `--with-dbm`
help string still names only "BerkeleyDB or KyotoCabinet", so Debian is
the first distro to select it, by its own choice rather than an upstream
default that leaked. Debian stable is not affected — trixie and bookworm
still carry `2.8.1-1` on Berkeley DB — but the switch is real and
released into a rolling suite, which is exactly the event this note was
written to get ahead of.

The full backend matrix, measured 2026-08-29 with
`tools/tkrzw/libpinyin-backend-probe.sh` — it reads each distro's
installed `libpinyin.so` `DT_NEEDED` entries (`readelf`/`objdump`) for
the directly linked storage library — and corroborated against each
build recipe (`debian/rules`, Fedora `libpinyin.spec`, Arch `PKGBUILD`,
nixpkgs `package.nix`):

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

Three families, three backends: the RPM/Arch/Nix world is uniformly on
Kyoto Cabinet, the `.deb` world was on Berkeley DB, and Debian has now
moved its rolling suites to tkrzw. No *shipped* cell is on tkrzw **and**
broken at once — yet. Debian's own tkrzw is healthy (the tkrzw matrix
later in this note), so a Debian sid user gets the switch without the
defects, losing only the old dictionary the NEWS warns about. The
dangerous cell is empty by timing alone: **Ubuntu's tkrzw is broken on
both defects, and Ubuntu tracks Debian.** The release that syncs
`libpinyin 2.11.91`'s `--with-dbm=Tkrzw` onto Ubuntu's own libtkrzw puts
defect 1 on every user dictionary — a `Remove` that stores the sentinel
instead of deleting, silently. This is no longer "before anyone flips
the switch": the switch is flipped upstream of Ubuntu, and only Ubuntu's
release cadence is holding it off.

The exposure is not what the file layout suggests:

- **The comparator fault does not reach libpinyin**, even though two of
  its five tkrzw backends use TreeDBM. It has no `OpenAdvanced`, no
  `TuningParameters` and no `key_comparator` in `src/` at all, so the
  comparator stays `nullptr` and is resolved inside libtkrzw where both
  sides of the comparison are the same copy — row (a) of the probe.
- **The sentinel fault does.** Three `Remove()` call sites —
  `ngram_tkrzwdb.cpp:140` (`Bigram::remove`),
  `punct_table_tkrzwdb.cpp:157`, and `flexible_ngram_tkrzwdb.h:263`
  (the user n-gram) — would leave the record in place carrying the
  five-byte `REMOVE` sentinel as its value. That is user-dictionary
  data.
- The NOOP-returning processors are safe as written:
  `KeyCollectProcessor` and `FlexibleKeyCollectProcessor` both run
  under `ProcessEach(&processor, false)`, and the read-only traversal
  never writes a return value back.

Which is the general shape of the hazard. The sentinel fault is not a
client/library boundary problem at all — `dbm.Remove(key)` passes no
sentinel across anything, and still corrupts, because LTO duplicated
the literal *inside* libtkrzw. Any consumer is exposed, in any
language binding, whether or not it ever names a sentinel. The
comparator fault is the narrower one: it needs a caller that names a
built-in comparator itself, which is why `tkrzw_dbm_util` trips it and
libpinyin would not.

## What this means for us

Nothing in `oxpinyin` changes. The tkrzw backend is an evaluation
subject behind an off-by-default cargo feature. `build.rs` requires
tkrzw to be discoverable through `pkg-config` — when it is absent the
build fails with a message that points at a source build and spells out
the Ubuntu hazard — but it does **not** verify the origin or health of a
tkrzw that `pkg-config` does find: a broken distro `libtkrzw-dev` on
`PKG_CONFIG_PATH` would link, which is what `tools/tkrzw/distro-probe.sh`
is for. What changed is the guidance: the warning named noble's
`1.0.27-1.1build1`, and the fault is neither version- nor
release-specific.

The quickest check on a candidate library, needing no build and no test
database:

```sh
readelf -rW /usr/lib/*/libtkrzw.so.1 | grep -c KeyComparator   # 0 = defect 2
```

`tools/tkrzw/distro-probe.sh` wraps that plus a write-then-read round
trip; `tools/tkrzw/identity-probe.cc` exercises all three
pointer-identity sites through the library API, which is what a backend
actually touches. Both exit non-zero on a broken build.

## Reported

Tracked in Ubuntu as [LP #2142937][lp], filed 2026-02-28 by Georgi
Georgiev against `src:tkrzw`, from the HashDBM `remove` symptom on
Ubuntu 25.10. The report correctly identifies it as Ubuntu-specific and
shows Debian trixie's `1.0.32-1+b1` behaving correctly.

Two things about it are worth knowing before adding anything. It is
still New / Undecided / Unassigned six months on, and it was filed
against **25.10, which reached end of life in July 2026** — an Ubuntu
bug whose only reproducer is an EOL interim release is easy to leave
alone. The measurements above answer that directly: the same faults are
present, unchanged, on 24.04 LTS and 26.04 LTS, and the development
series still ships resolute's binary, so nothing has been fixed
anywhere. That, rather than a re-explanation of the mechanism, is what
the report is missing.

**The patch attached there is incomplete.** It sets

```make
export DEB_BUILD_MAINT_OPTIONS = hardening=+all optimize=-lto
```

which is row B of the bisection: `remove` is fixed, and every TreeDBM
file the library writes remains unreadable by the library itself. The
complete fix needs both lines:

```make
export DEB_BUILD_MAINT_OPTIONS = hardening=+all optimize=-lto
export DEB_LDFLAGS_MAINT_STRIP = -Wl,-Bsymbolic-functions
```

That is row D, which matches the Debian reference build exactly.

Both lines are no-ops on Debian, which enables neither flag, so the
natural home for them is Debian's `debian/rules`, from which Ubuntu
syncs — no permanent Ubuntu delta to carry. Two things temper that
route: `src:tkrzw` is orphaned in Debian (`Maintainer: Debian QA Group
<packages@qa.debian.org>`, with Boyuan Yang doing QA uploads through
`1.0.32-1`), so it may need an NMU or a merge request against
`salsa.debian.org/debian/tkrzw`; and a sync reaches no released Ubuntu
LTS, so 24.04 and 26.04 need SRUs through the Launchpad bug regardless.

That sync is no longer hypothetical. Boyuan Yang — the same maintainer
doing tkrzw's QA uploads — switched Debian's `libpinyin` to
`--with-dbm=Tkrzw` in `2.11.91-1` (unstable, 2026-08-12). The two
threads now meet in one archive: a libtkrzw that is healthy in Debian
and broken in Ubuntu, and a libpinyin that has begun writing user
dictionaries through it. Fixing the tkrzw build before Ubuntu's next
libpinyin sync is what keeps defect 1 off those dictionaries; after the
sync, the Launchpad bug stops being about a command-line tool.

## Upstream

> **Amended 2026-10-05.** Defect 1 was fixed upstream on 2026-09-20, in
> `9ee8416` (release 1.0.34), by neither remedy proposed below. The
> check is still `value.data() == NOOP.data()`, an address identity;
> what changed is that each sentinel is backed by one named array
> instead of a string literal, so there is exactly one address to
> compare against. That is enough: an LTO build of 1.0.34 is healthy.
> Defect 2 is untouched by that commit. See the last section.

Upstream is unfixed as of `bcaa0fb` (last commit 2026-07-30, so it is
actively maintained). Both defects are the same underlying decision:
identifying a value by an address that C++ only guarantees under
default ELF interposition and a single definition. A distro flag, LTO
partitioning, a static link of one side, or `dlopen` with `RTLD_LOCAL`
all break it, and each breaks it silently and on disk; on Windows, where
each module gets its own copy of an inline function by default, it is
hard to see how it can hold at all.

The fix is not to move the comparators out of the header. A function
defined only in the shared library still fails under
`-Bsymbolic-functions`: the library binds to its own definition while
the client's address-taking yields a canonical PLT entry in the client.
Nothing that keeps identifying a comparator by its address is safe. A
one-byte enum in `TuningParameters` alongside the pointer costs nothing
and is immune — and the on-disk format is already an enum byte, so only
the derivation from a pointer has to change. The sentinels want the same
treatment: a tagged return, or a comparison on contents rather than on
`data()`.

This is worth reporting upstream separately from the Ubuntu bug, since
it is a different ask; it is not a Rust-mechanism divergence, so it does
not belong in `upstream-divergences.md`.

## The matrix, measured

Run on 2026-08-29 with `tools/tkrzw/distro-probe.sh`, one rolling
container per distro (podman; `:latest` / `testing` tags, deliberately
unpinned), across the three packaging families. Defect 1 is the
LTO/`remove` sentinel; defect 2 is the `-Bsymbolic-functions`/comparator.

The tags are rolling by design (the newest toolchain a distro ships is a
property under test), so for reproducibility the images behind this and
the backend matrix resolved on 2026-08-29 to these digests:

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

- **Ubuntu 26.04 LTS** reproduces both defects exactly as the release
  row above predicts: `remove` stores the sentinel, the `.tkt`
  comparator records type 255 and the file will not reopen, and the
  library keeps zero comparator GOT relocations. `1.0.32-1build1`, live
  from the archive.
- **Debian testing** is healthy — the flags prediction, and the same
  clean result the Launchpad reporter saw on trixie. `1.0.32-1+b2`.
  `src:tkrzw` is orphaned in Debian (maintained by the Debian QA Group),
  so it is QA-owned rather than actively maintained — the same status
  the fix route runs into under "Reported".
- **Fedora Rawhide** is healthy, and is the most instructive row.
  Fedora enables LTO by default, so its tkrzw would carry defect 1 like
  any LTO build — except the package escapes it deliberately:
  `tkrzw.spec` sets `%global _lto_cflags %{nil}`, with the changelog
  reason *"Disabled LTO, since it causes test failures on all
  file-based database tests."* That is an independent maintainer hitting
  defect 1 through the package's own `make check` and turning LTO off to
  get a working build — outside corroboration of the bisection above,
  from a distro that had every reason to keep LTO on. Fedora adds no
  `-Bsymbolic-functions`, so defect 2 never arises. `1.0.32-5.fc45`.
- **RHEL 10.2** carries no tkrzw in its *own* repositories — `dnf
  install tkrzw` on `redhat/ubi10:latest` returns *"No match for
  argument: tkrzw"* across BaseOS, AppStream and CodeReady Builder — but
  **EPEL 10 ships it**, `tkrzw-1.0.32-2.el10_1` (vendor Fedora Project),
  and that build is **healthy**: 18 comparator relocations, `remove`
  clean, comparator type 1. EPEL follows the Fedora spec lineage, so it
  inherits the `%_lto_cflags %{nil}` opt-out, and RHEL adds no
  `-Bsymbolic-functions` — neither defect arises. A tkrzw backend built
  against EPEL's libtkrzw on the RHEL 10.2 drop-in target is therefore
  safe. Measured on that target itself (`crb`/EPEL enabled).
- **Arch** ships no tkrzw in `core`/`extra` either (`pacman -Ss tkrzw`
  is empty); the only Arch source is the AUR `tkrzw-git`, a stale 2020
  VCS stub (0 votes, building upstream HEAD with a plain
  `./configure && make`). With nothing packaged to probe, tkrzw 1.0.32
  built from the upstream release under Arch's stock `makepkg.conf`
  (gcc 16.2.1; `OPTIONS=(... lto)`, no `-Bsymbolic-functions`)
  reproduces row C of the bisection precisely — `remove` broken,
  comparator intact. Arch has no distro-wide `_lto_cflags` opt-out, so a
  tkrzw built there is exposed to defect 1 unless its own PKGBUILD
  disables LTO, which neither the AUR stub nor a plain build does.

This supersedes the earlier "still unrun" note: the mirrors that
returned 403 in the originating session are reachable here, and the two
`.deb` rows, Fedora and RHEL-via-EPEL were installed straight from each
archive. Arch is the one package-absence row, its defect measured from a
source build under the distro's own flags.

`distro-probe.sh` covers both halves, and was checked against all four
rows of the bisection: it reports A broken on both, B broken on the
comparator only, C broken on `remove` only, and D healthy. A distro that
prints `RESULT : healthy` has neither defect.

## tkrzw 1.0.34 fixes defect 1 upstream

Measured 2026-10-01 (UTC) at `c99ffb57`, and again 2026-10-05 at
`e985b581`, after #621 (`eaeb26fd`, merged 2026-10-03) moved the
libpinyin session into memory. The second measurement changes what a
defect-1 library does to this backend, so the two are kept apart below.
Branch `claude/tkrzw-134-sentinel-finding`.

Upstream changed how the sentinels are stored on 2026-09-20:
`estraier/tkrzw` commit `9ee8416`, "Change the object format of
DBM::RecordProcessor::NOOP and DBM::RecordProcessor::REMOVE" — ChangeLog
"Fix bugs of address instability of NOOP and REMOVE", release 1.0.34,
library 1.77.0. It answers the sentinel half of "Upstream" above by a
third route, and for defect 1 it is complete: an LTO build of 1.0.34 is
healthy. Defect 2 is not touched.

This section exists because that commit first arrived here as a
suspect. A run of the tkrzw cell against a from-source `9ee8416` was
reported on 2026-10-01 with `pinyin_save` returning `false` after any
import or train on both C ABIs, the pin-built libpinyin saving fine
against the same library, and a rebuild at the 1.0.32 state (`bcaa0fb`)
passing — and the commit's "object format" change was taken to be the
regression. **That attribution does not hold, and the failure did not
reproduce**: oxpinyin passes against every build of `9ee8416` measured
below, on two architectures. The symptom itself is real and exact,
though. It is what a *defect-1* library did to this backend as it stood
that day — the fault `9ee8416` removes. The reported run's environment
was not examined, so which libtkrzw it actually loaded is not
established here; the inversion is recorded as unexplained rather than
explained away.

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

Same type, same five bytes, same size. What changes is only what each
`string_view` points at: an anonymous string literal, which GCC may
materialise once per LTO partition, becomes a named array, which is one
object with one address. `DBM::ANY_DATA` gets the same treatment.

Nothing a client is compiled against moves:

- `tkrzw_langc.h`, `tkrzw_langc.cc` and `tkrzw_dbm.h` are the same git
  blobs at both commits (`7f25a305`, `fc8e56ae`, `61d13d2d`), and the two
  installed include trees are byte-identical.
- The C API's sentinels were never the C++ ones.
  `TKRZW_REC_PROC_NOOP` and `TKRZW_REC_PROC_REMOVE` are `(char*)-1` and
  `(char*)-2` (`tkrzw_langc.cc:60,62`) — tags, the address of nothing —
  and both built libraries read back `0xffffffffffffffff` and
  `0xfffffffffffffffe`.
- The callback contract is the same: a `tkrzw_record_processor`
  (`tkrzw_langc.h:144-153`) returns one of the two tags or a pointer
  whose length it stores through its last argument, and
  `RecordProcessorWrapper` (`tkrzw_langc.cc:64-91`) compares that return
  against the tags by value before substituting the C++ sentinel.

So `oxpinyin-store`'s tkrzw backend needs no change to work with both
versions and no means of telling them apart: it reads the two tags from
the library's own globals and hands them back, and everything after that
happens inside libtkrzw on either version. A version gate that refused
1.0.34 would refuse the one release that is immune to defect 1.

`bcaa0fb..9ee8416` holds two more commits, and neither is involved.
`9db46db` (the 1.0.33 change) frees a leaked iterator key buffer in
`TreeDBM` and `BabyDBM`, reachable only by keys longer than 128 bytes;
`17926f7` is the version number and a regenerated `configure` that emits
the same compiler and linker flags.

Upstream was read from a fresh clone of `https://github.com/estraier/tkrzw`
at those commits; the blob ids are `git rev-parse <commit>:<file>`.

### Measured 2026-10-01, at `c99ffb57`

oxpinyin at `c99ffb57`: a libpinyin session still ran on a file-backed
`TreeDBM` scratch, and the store suite counted 39 unit tests with this
section's test in its first, three-class form.

Each commit built with `./configure --prefix=… && make && make install`
in `debian:testing` (g++ 16.2.0) under three flag sets: **plain**
(tkrzw's defaults, `-g -O2`), **LTO** (`-flto=auto -ffat-lto-objects`;
LTO alone, as on Arch) and **Ubuntu** (LTO plus
`-Wl,-Bsymbolic-functions`). arm64:

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

"probe" is `tools/tkrzw/identity-probe.cc`; the store suite is
`oxpinyin-store`'s unit tests plus `tests/trait_laws.rs`; the two saves
are the `save:` line of `tools/bisection/import-diff.c` and
`zhuyin-import-diff.c` driven into oxpinyin's C ABI objects and into the
pin-built libpinyin and libzhuyin, each loading the build in that row.
Wherever both saves read `true` the two logs are identical, and so are
the two `nbest-train-diff` logs (the train path); where oxpinyin reads
`false`, `nbest-train-diff` stops at `reopen: pinyin_save failed` and the
pin's run completes. `distro-probe.sh` agrees with the probe columns on
all eight builds. (`9db46db` reports itself as 1.0.32: its checked-in
`configure` was not regenerated until `17926f7`.) Debian testing's own
package, `1.0.32-1+b2` — the library CI's tkrzw child links — passes the
same 39 + 6.

On the two plain builds the wider gates pass as well: the tkrzw child of
`store-backends.yml` (101 suites, 1076 passed, 0 failed, 16 ignored —
the same counts on `bcaa0fb` and `9ee8416`),
`tools/oracle/user-dir-round-trip.sh` end to end (Phase C IDENTICAL,
Phase D READABLE, its system-import phase IDENTICAL for pinyin libraries
1–4) and `tools/bisection/run-system-import-round-trip.sh zhuyin`
(IDENTICAL, libraries 1–4).

The same four cells on amd64 (the same image under emulation; the pin
prefix is arm64, so oxpinyin's side only) and the plain pair on macOS
(arm64, Apple clang 21.0.0, store suite only):

| tkrzw | flags | host | `.rodata` copies | probe (c) (d) (e) | store suite | `pinyin_save`, `zhuyin_save` |
| --- | --- | --- | --- | --- | --- | --- |
| 1.0.32 `bcaa0fb` | plain | amd64 | 1 / 1 | ok | 39 + 6 pass | `true` |
| 1.0.32 `bcaa0fb` | LTO | amd64 | 9 / 12 | **broken** | **5 of 39 fail** | **`false`** |
| 1.0.34 `9ee8416` | plain | amd64 | 1 / 1 | ok | 39 + 6 pass | `true` |
| 1.0.34 `9ee8416` | LTO | amd64 | 1 / 1 | ok | 39 + 6 pass | `true` |
| 1.0.32 `bcaa0fb` | plain | macOS | – | – | 39 + 6 pass | – |
| 1.0.34 `9ee8416` | plain | macOS | – | – | 39 + 6 pass | – |

Three things follow.

**Defect 1 is fixed at the mechanism, not masked.** The LTO build of
1.0.32 carries nine and ten copies of the two literals in `.rodata`
(nine and twelve on amd64); the LTO build of 1.0.34 carries one of each,
on both architectures.

**Defect 2 is where it was.** With Ubuntu's flags 1.0.34 still keeps no
GOT relocation for the comparators and still fails probe row (b). That
does not reach this backend, which installs no comparator: its whole
suite passes on that build, and its logs match the pin's. On that
evidence a 1.0.34 built with Ubuntu's two flags is sound for oxpinyin
and for libpinyin, and still unsound for `tkrzw_dbm_util` and any client
that names a comparator. Ubuntu's own package, when it has one, still
has to be probed rather than assumed.

**The literal count needs a section now.** An unstripped 1.0.34 holds
each five-byte pattern twice in the file — once in `.rodata`, once in
`.debug_info` as the named array's constant value — so the whole-file
count under "Defect 1" reads 2 on a healthy build. Count within
`.rodata`.

### What a defect-1 library did at `c99ffb57`

The row that matters is 1.0.32 with LTO, because it is the reported
symptom line for line:

- `pinyin_save` and `zhuyin_save` returned `false` after an import or a
  train, and nothing was logged. In the `import-diff` log the `save:`
  line was the only line that differed from a healthy run.
- The pin-built libpinyin, loading the same library, returned `true`.
  Its tkrzw sources never call `Rebuild` (`src/storage/*tkrzw*` at
  `074a2219`), so nothing on its save path can fail this way. Its three
  `Remove()` sites are where the fault would land instead, silently, as
  described above — read from the source, not measured in the pin.
- The error oxpinyin swallowed was `tkrzw status 6`
  (`TKRZW_STATUS_CANCELED_ERROR`, empty message) from
  `tkrzw_dbm_rebuild`, reached through `TkrzwStore::compact`, which
  `UserStore::save` called on the session's scratch file after it had
  written the profile. `ContextCore::save_user` turns any `Err` into
  `false`, which is also the ABI's answer for "nothing modified" and "no
  user dir", so the three cannot be told apart from outside. The rebuild
  cancels because it replays every record through a processor that
  answers `NOOP` (`tkrzw_dbm_hash.cc:1914-1934`) and takes any other
  answer as a cancellation (`tkrzw_dbm_hash_impl.cc:427-428`, the only
  place the DBM classes produce that status) — the same `data()`
  comparison, between two partitions' copies of the literal. `TreeDBM`
  rebuilds through its `HashDBM` (`tkrzw_dbm_tree.cc:907`), so both file
  classes fail alike.
- The profile was on disk by then: after `save: false` the user dir held
  the same eleven files, at the same sizes, as after a successful save.
  The `false` was the session scratch store failing to compact, not the
  write failing.
- Removal is wrong as well, and silently. Removing an absent record
  stores one whose value is `NOOP`'s five bytes (`00 be ef 02 11`);
  removing a stored record leaves it in place carrying `REMOVE`'s
  (`00 de ad 02 11`); both writes report success. That held on all
  three DBM classes the backend then opened (`TreeDBM`, `HashDBM`, the
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

and `tests::tkrzw_library_honours_its_sentinel_protocol`, added with
this section, says so in words and names this document. In its first
form it passed on the six healthy builds above and failed on the two
defect-1 builds.

The C API does not shield the backend from any of this, and the note in
"It is not only the CLI" already says why: the sentinel fault is inside
libtkrzw, between the wrapper that returns the C++ sentinel and the DBM
class that compares it. Reading the tags from the library's globals
guarantees agreement at the boundary and nothing beyond it.

### What #621 changed — re-measured 2026-10-05, at `e985b581`

#621 (`eaeb26fd`) took the libpinyin session off its temp file. It now
runs on `WriteStore::create_in_memory`, which on this backend is tkrzw's
`BabyDBM` through PolyDBM's empty-path form — a fourth DBM class beside
`TreeDBM`, `HashDBM` and the user bigram's `TinyDBM`.

**The save path still reaches `tkrzw_dbm_rebuild`, and the call can no
longer fail.** Every shipping save — `pinyin_save`, `zhuyin_save`,
`oxpinyin-dictool import` — goes through `ContextCore::save_user`
(`crates/oxpinyin-facade/src/context.rs:284-287`) into
`GenericUserStore::save`, which still ends by compacting the session
store (`crates/oxpinyin-user/src/store.rs:1516`), and
`TkrzwStore::compact` still calls `tkrzw_dbm_rebuild`. On a `BabyDBM`
that is a no-op: `PolyDBM::RebuildAdvanced` falls through to
`BabyDBM::Rebuild` (`tkrzw_dbm_poly.cc:755`), whose whole body is
`return Status(Status::SUCCESS)` under the comment "This method does
nothing" (`tkrzw_dbm_baby.h:284-290`, blob `0b934760` at both commits).
The only other `compact` call outside tests, benches and examples is the
default `WriteStore::write_user_bigram`
(`crates/oxpinyin-store/src/lib.rs:419`), on a `HashDBM` file, and no
shipping path reaches it: `persistence::save_with_bigram` takes that
branch only without a bigram container, a libpinyin session always
passes its own, and `persistence::save`, which passes none, has test
callers only. A store opened on a file path (`UserStore::open`,
`create_standalone`) would still compact a `TreeDBM`, and only tests,
benches, examples and the fuzz target open one. So the record replay
that cancels — the mechanism behind every `false` in the table above —
is out of reach of a shipping save.

**The defect is still in reach, through removal.** The same drivers at
`e985b581`, each side loading the build in its row, and
`tools/bisection/abi-probe-diff.c` (sort word `1e`), which saves after
an import and again after a `pinyin_mask_out`:

| tkrzw | flags | store suite | `import-diff`, `zhuyin-import-diff` save | `nbest-train-diff` | `abi-probe-diff` save after import, after `mask_out` | the pin, same four |
| --- | --- | --- | --- | --- | --- | --- |
| Debian `1.0.32-1+b2` | package | 40 + 6 pass | – | – | – | – |
| 1.0.32 `bcaa0fb` | LTO | **6 of 40 fail** | `true` | completes | `true`, **`false`** | `true`, completes, `true`, `true` |
| 1.0.34 `9ee8416` | LTO | 40 + 6 pass | `true` | completes | `true`, `true` | `true`, completes, `true`, `true` |

On the LTO 1.0.32 the first three drivers' logs are now identical to the
pin's — the `false` of 2026-10-01 is gone — and the fourth differs in
three observations: `save(after-mask)`, `false` against the pin's
`true`, and, in the context it then opens on the same user dir,
`lookup_tokens(你好)` (two tokens against one) and the token
`phrase_token[0]` reports, because the mask that could not be saved is
not in the profile. On the LTO 1.0.34 all four logs are identical.

This is the removal fault of the list above, now inside the session.
`GenericUserStore::mask_out` issues `WriteTxn::remove` on the session
store — as `remove_user_phrase` does behind
`pinyin_remove_user_candidate`, and as a train that empties a
predecessor's row does on the user bigram's `TinyDBM`. On a defect-1
library `pinyin_mask_out` returns `true` while the rows it removed stay
in the `BabyDBM`, carrying `REMOVE`'s five bytes. The next save that has
something to write — the probe's context was already modified — runs
`export_state`, which reads a pronunciation row that should be gone,
finds five bytes where a value is 8 or 13, and fails with
`corrupt pronunciation value`; `ContextCore::save_user` turns that into
`false`. The pin, on the same library, answers `true`.

So since #621 a defect-1 library is quieter on this backend, not safer.
A save after an import or a train succeeds and matches the pin; the
first removal breaks the session, and no call says so until the next
save with something to write. #677 tracks it and carries the sequence
save by save.

In the store suite the in-memory class adds a sixth failure on the LTO
1.0.32, `tests::tkrzw_write::in_memory_store_behaves_as_a_tree_store`
(`left: Some([0, 222, 173, 2, 17])`), and this section's test now checks
four classes. On that library `BabyDBM` stores a record for an absent
removal and keeps a removed one, as the other three classes do, and its
`compact` succeeds.

### The removal behind `save(after-mask)`, call by call

Three facts for whoever takes that failing save further; they are the
first two things #677 asks to have established before a fix. Nothing
here is implemented: how the store removes a record is shipped code, and
changing it is that issue's work.

**The oxpinyin call that relies on the sentinel.** The rows that outlive
the mask are the masked phrases' pronunciation rows, which
`GenericUserStore::mask_out` removes with `txn.remove(PRONUNCIATION, …)`
at `crates/oxpinyin-user/src/store.rs:1724`, after the bigram, unigram
and phrase rows at `:1643-1718`. The tkrzw backend only buffers that
(`TkrzwWriteTxn::remove`, `crates/oxpinyin-store/src/tkrzw/mod.rs:1012-1016`).
The call that relies on the sentinel comes at commit: `db_apply` hands
the batch to `tkrzw_dbm_process_multi` (`mod.rs:741`), whose callback
`apply_one` answers `TKRZW_REC_PROC_REMOVE` for a record that exists
(`mod.rs:569`) and `TKRZW_REC_PROC_NOOP` for one that does not (`:567`).
libtkrzw's wrapper turns the tag into the C++ `REMOVE`, and `BabyDBM`
compares that by address (`tkrzw_dbm_baby.cc:1081-1103`). The user
bigram's `TinyDBM` takes the same route (`TkrzwUserBigramDb::remove`,
`mod.rs:1141`, through `db_set_one`, `:1078`). The next save reads the
surviving row back at `crates/oxpinyin-user/src/store_libpinyin.rs:441`.
All at `e985b581`.

**How the pin removes the same record.** It does not ask tkrzw to: the
record is not in tkrzw. A phrase's pronunciations live in its
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
on any class the store uses. `tkrzw_dbm_remove` is `PolyDBM::Remove`
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

The pin's own exposure is that one call: its user-bigram `Remove`
(`ngram_tkrzwdb.cpp:140`) is `DBM::Remove` on a `TinyDBM` and goes
through the same sentinel — one of the three `Remove()` sites under
"Does anything actually ship against this?". **That is inferred, not
measured.** It rests on the probe above, whose `TinyDBM` case reaches
the same `DBM::Remove` through the C API, and on the source. The pin was
not driven through `Bigram::remove` on the LTO 1.0.32: the
`abi-probe-diff` run observed its `pinyin_mask_out` and both of its
saves answering `true` and nothing below the ABI, so whether that
mask-out reached the call, and what the pin's user bigram held
afterwards, are not established here. Where the pin's mask-out differs
from oxpinyin's is in where the masked records live, not in how tkrzw
removes them: its phrase items are not tkrzw records, and its index
records are rewritten rather than removed.

### If `pinyin_save` returns `false` on tkrzw

Which saves fail depends on the tree. Before #621 (`eaeb26fd`): any save
after an import or a train, while the pin saves `true`. From #621 on: no
save fails until the context removes something — `pinyin_mask_out` is
the measured case — and the next save with something to write does,
while the pin still saves `true` (#677). On `e985b581` neither failure
is logged; save's diagnostics and return value belong to #545 (PR #666
when this was written), and nothing below depends on them.

In both cases check the build of the library, not its version; below
1.0.34 the version says nothing.

1. **Find the libtkrzw the failing process loaded.** It need not be the
   one the build linked. `oxpinyin-store`'s build script puts a RUNPATH
   on that package's own targets (its unit-test, `trait_laws` and example
   binaries, by `readelf -d`) and on nothing else — `rustc-link-arg` is
   package-scoped. `libpinyin_capi.so`, `libzhuyin_capi.so` and the other
   crates' test binaries (`oxpinyin-user`, `-data`, `-runtime` checked)
   carry none, and take whichever `libtkrzw.so.1` `LD_LIBRARY_PATH` or the
   loader cache names. `tools/bisection/run-system-import-round-trip.sh` also
   replaces `LD_LIBRARY_PATH` with the directory of the object under
   test, so there the loader cache decides for both sides. A store suite
   that passes and a C-ABI driver that fails in the same shell can be
   two different libraries. `ldd`, or `LD_DEBUG=libs` on the failing
   command, settles it.
2. **Probe that library.** Build `tools/tkrzw/identity-probe.cc` against
   it (rows (c)–(e)), or run this backend's own check:

   ```sh
   PKG_CONFIG_PATH=$P/lib/pkgconfig LD_LIBRARY_PATH=$P/lib \
     cargo test -p oxpinyin-store --no-default-features --features tkrzw \
       tkrzw_library_honours_its_sentinel_protocol
   ```
3. **Then read the version.** 1.0.34 passed under every flag set
   measured here. 1.0.33 and earlier are healthy or not according to how
   they were built: fine from `./configure && make` and from Debian,
   Fedora and EPEL; broken from Ubuntu and under Arch's default flags.

Debian testing still carried `1.0.32-1+b2` on 2026-10-05, as on
2026-10-01 (`apt-cache policy libtkrzw-dev`); no other distro row of
this note was re-measured. CI runs this backend's suite, and with it the
test above, in three places: `store-backends.yml`'s tkrzw child and its
two sanitizer arms, all in `debian:testing` on that package, and
`ci.yml`'s `test-macos` on Homebrew's bottle. No job links an Ubuntu- or
Arch-built libtkrzw. Homebrew's formula was at 1.0.34 on both dates
(`brew info tkrzw`), while the hosted macOS runner still poured the
1.0.32 bottle on 2026-10-01 (`test-macos`, run 36832757541). The suite
passes against the 1.0.32 bottle on the measuring host (macOS 27.0.1,
arm64, 2026-10-05), and the lane pins only the 1.x line, so it will move
to 1.0.34 on its own.

### Provenance — the command behind each figure

2026-10-01 (UTC), on an arm64 macOS host in Docker: `debian:testing` at
`debian@sha256:16faa8d1cd99fcb2d30eebe90454e26b20499055fa7094e9b55d32d1a7666f08`
(native, and `--platform linux/amd64` for the amd64 rows), g++ 16.2.0
(Debian 16.2.0-3), GNU ld 2.47, rustc 1.97.1; apt set = the
`store-backends.yml` test job's without `libtkrzw-dev`,
`libkyotocabinet-dev` and `libdb-dev`, plus `python3` and `gdb`, so no
distro libtkrzw existed in either container and a binary that missed the
build under test could not fall back to another. oxpinyin at `c99ffb57`
plus this section's change in its first form.

2026-10-05 (UTC), the same host under Apple `container` 1.5.0: the same
image digest, arm64 native, the same g++, ld and rustc; apt set = that
test job's in full, plus `python3` and `gdb`. Debian's `libtkrzw-dev
1.0.32-1+b2` therefore sat beside the two from-source LTO builds, and
every run was checked for the libtkrzw it loaded (the row below).
oxpinyin at `e985b581` plus this change.

`$P` is one build's prefix, `$T` a `CARGO_TARGET_DIR` of its own per
build, `$ORACLE` a prefix from `tools/oracle/build-oracle.sh` (libpinyin
2.11.92 at `074a2219`, `--with-dbm=Tkrzw`, with libzhuyin). The oracle
was built once, on 2026-09-25 against Debian testing's 1.0.32 package,
and not rebuilt per row: it reaches each build through the loader, which
the identical headers make sound.

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
| probe rows | `g++ -std=c++17 -O2 -I $P/include tools/tkrzw/identity-probe.cc -o probe -L $P/lib -Wl,-rpath,$P/lib -ltkrzw -lpthread && ./probe DIR` |
| `distro-probe.sh` | `PATH=$P/bin:$PATH LD_LIBRARY_PATH=$P/lib sh tools/tkrzw/distro-probe.sh` |
| store suite | `PKG_CONFIG_PATH=$P/lib/pkgconfig LD_LIBRARY_PATH=$P/lib CARGO_TARGET_DIR=$T cargo test --locked --no-fail-fast -p oxpinyin-store --no-default-features --features tkrzw` |
| store suite, Debian package | the `store-backends.yml` test job's apt line in a fresh `debian:testing`, then `RUSTFLAGS="-D warnings" cargo test --locked --no-fail-fast -p oxpinyin-store --no-default-features --features tkrzw`; the package is `dpkg-query -W libtkrzw-dev libtkrzw1t64` |
| store suite, macOS | `env -u PKG_CONFIG_PATH PKG_CONFIG_LIBDIR=/nonexistent OXPINYIN_TKRZW_INCLUDE_DIR=$P/include OXPINYIN_TKRZW_LIB_DIR=$P/lib OXPINYIN_TKRZW_LIB_NAME=tkrzw CARGO_TARGET_DIR=$T cargo test …` as above; pkg-config is pointed away because a from-source `tkrzw.pc` names `-lstdc++`, which macOS does not have, on both commits |
| each class, each fault | the test stops at its first failed assertion — on the LTO 1.0.32, `TreeDBM`'s absent removal — so the other class-and-fault pairs were read from scratch copies of `crates/oxpinyin-store/src/lib.rs`, never committed: the class under study moved ahead of the others, the assertions before the one under study deleted, each copy run with the store-suite command and the test's name as its filter. Both dates; the 2026-10-05 copies add `BabyDBM` |
| workspace sweep | the store suite's three variables and `cargo test --locked --workspace --no-default-features --features tkrzw --exclude oxpinyin-corpus --exclude oxpinyin-counter --exclude oxpinyin-emitter --exclude oxpinyin-kmm --exclude oxpinyin-lambda --exclude oxpinyin-punct --exclude oxpinyin-word --no-fail-fast`, the `store-backends.yml` tkrzw child's command at `c99ffb57`; the totals are `awk '/^test result:/ {p+=$4; f+=$6; i+=$8; n++} END {print n, p, f, i}'` over its output |
| C ABI objects | the same three variables, `cargo build --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi --no-default-features --features tkrzw` |
| `pinyin_save` | `gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o import-diff tools/bisection/import-diff.c -ldl`, then `LD_LIBRARY_PATH=$P/lib ./import-diff $T/debug/libpinyin_capi.so $ORACLE/lib/libpinyin/data` |
| the pin's save | `LD_LIBRARY_PATH=$P/lib:$ORACLE/lib ./import-diff $ORACLE/lib/libpinyin.so $ORACLE/lib/libpinyin/data` |
| `zhuyin_save`, both sides | `cc -std=gnu11 -Wall -Wextra -Werror -O2 tools/bisection/zhuyin-import-diff.c $(pkg-config --cflags --libs glib-2.0) -ldl -o zhuyin-import-diff`, then, in an empty directory, `LD_LIBRARY_PATH=$P/lib ./zhuyin-import-diff $T/debug/libzhuyin_capi.so $ORACLE/lib/libpinyin/data 1`; the pin's form is `$ORACLE/lib/libzhuyin.so` with `:$ORACLE/lib` on the library path |
| train path | `gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o nbest-train-diff tools/bisection/nbest-train-diff.c -ldl`; `cp -r $ORACLE/lib/libpinyin/data sysdir`, model20's `interpolation2.text` copied into it; then `LD_LIBRARY_PATH=$P/lib ./nbest-train-diff $T/debug/libpinyin_capi.so sysdir`, and the pin's form on `$ORACLE/lib/libpinyin.so` |
| log identity, oxpinyin against the pin | each log is a driver's stdout: `diff <(sort pin.log) <(sort ox.log)` for `import-diff`, plain `diff pin.log ox.log` for `zhuyin-import-diff`, `nbest-train-diff` and `abi-probe-diff`. "The only line that differed from a healthy run" is `diff` of oxpinyin's `import-diff` stdout on the plain and on the LTO build of `bcaa0fb` |
| round trips | `echo $P/lib > /etc/ld.so.conf.d/00-tkrzw-under-test.conf && ldconfig`, then `PKG_CONFIG_PATH=$P/lib/pkgconfig CARGO_TARGET_DIR=$T tools/oracle/user-dir-round-trip.sh $ORACLE` and `tools/bisection/run-system-import-round-trip.sh zhuyin $ORACLE/lib/libzhuyin.so.15 $T/debug/libzhuyin_capi.so $ORACLE/lib/libpinyin/data` |
| the swallowed error | the one-line scratch patch printed below the table, at `c99ffb57`; the C ABI objects rebuilt against the LTO 1.0.32 into a `$T` of their own; then the three drivers with `2>&1 \| grep -E 'save\|SCRATCH-DIAG'` |
| files after `save: false` | `ls -la /tmp/importdiff-user-*` after the `import-diff` run, which leaves its user dir there, on the LTO 1.0.32 and on the LTO 1.0.34 |
| what the pin calls | `git grep -nE 'Rebuild\|Remove\(\|Synchronize\|ProcessEach' 074a2219 -- 'src/storage/*tkrzw*'` |
| RUNPATH | `readelf -d <object> \| grep RUNPATH` on `oxpinyin-store`'s unit-test, `trait_laws` and example binaries, on the `oxpinyin-user`, `-data` and `-runtime` test binaries and on the two C ABI objects, all under `$T/debug` |
| environment | `g++ --version`, `ld --version` and `rustc --version` in the container; `sw_vers`, `clang --version` and `container --version` on the host; the image digest is `docker image inspect --format '{{index .RepoDigests 0}}' debian:testing` on 2026-10-01, and the 2026-10-05 container was started from that digest |
| 2026-10-05: store suite | the store-suite command at `e985b581`, without its first two variables for Debian's package |
| 2026-10-05: the library a run loaded | `ldd <test binary> \| grep tkrzw`; `LD_DEBUG=libs <driver> … 2>&1 \| grep 'calling init: .*libtkrzw'` |
| 2026-10-05: the three earlier drivers | as above, each run in an empty directory of its own |
| 2026-10-05: save after import, after `mask_out` | `gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o abi-probe-diff tools/bisection/abi-probe-diff.c -ldl`, then `LD_LIBRARY_PATH=$P/lib ./abi-probe-diff $T/debug/libpinyin_capi.so sysdir 1e` on the train path's `sysdir`, and the pin's form on `$ORACLE/lib/libpinyin.so` |
| 2026-10-05: the swallowed error after `mask_out` | the same scratch patch at `e985b581`; `libpinyin_capi.so` rebuilt against the LTO 1.0.32; then the `abi-probe-diff` command with `2>&1 \| grep -E 'SCRATCH-DIAG\|^save\|^mask_out'` |
| 2026-10-05: Homebrew | `LIBRARY_PATH=$(brew --prefix)/lib cargo test --locked --no-fail-fast -p oxpinyin-store --no-default-features --features tkrzw` on the macOS host (`brew list --versions tkrzw`: 1.0.32) |
| the hosted runner's bottle, 2026-10-01 | `gh run view 36832757541 --job 110272962477 --log \| grep 'Pouring tkrzw'` |
| 2026-10-05: `compact` callers | `git grep -nE '\.compact\(' e985b581 -- 'crates/*.rs'`, then the callers of `write_user_bigram`, `stage_user_bigram`, `persistence::save`, `UserStore::open` and `create_standalone` |
| 2026-10-05: tkrzw's plain remove, per class | the scratch program printed below the table, which is in no repository: `gcc -std=gnu11 -Wall -Wextra -O2 -I $P/include plain-remove-probe.c -o probe -L $P/lib -Wl,-rpath,$P/lib -ltkrzw`, then `./probe` in an empty directory; for Debian's package, the same without `-I`, `-L` and `-rpath` |
| 2026-10-05: the pin's mask-out | libpinyin at `074a2219` from a local mirror clone; blobs `f27f7cf7` (`src/pinyin.cpp`), `d58e37e2` (`phrase_index.cpp`), `8ae83e01` (`chewing_large_table2_tkrzwdb.cpp`), `0e4094fe` (`phrase_large_table3_tkrzwdb.cpp`), `c826a444` (`ngram_tkrzwdb.cpp`) by `git rev-parse 074a2219:<file>` |
| 2026-10-05: CI's libtkrzw per job | every `container:`, `runs-on:`, `libtkrzw`, `brew install` and `--features` line of `.github/workflows/*.yml` at `e985b581`, and the `cargo` lines of the scripts they call |

Two rows rest on scratch sources that are in no repository, so both are
printed here in full.

The swallowed error. One line of `ContextCore::save_user`
(`crates/oxpinyin-facade/src/context.rs`), the same line at `c99ffb57`
and at `e985b581`, changed for the measurement and never committed:

```diff
-            .is_some_and(|store| store.save().unwrap_or(false))
+            .is_some_and(|store| store.save().unwrap_or_else(|e| { eprintln!("SCRATCH-DIAG save error: {e} / {e:?}"); false }))
```

tkrzw's plain remove, per class — `plain-remove-probe.c`. This is a
shortened form of the program that first produced the per-class table,
re-run on 2026-10-05 against the same three libraries with the same
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

Per `docs/runbooks/benches.md`, this document commits no captures, and
the raw capture behind this section is not retained: no build, probe,
test or driver log of either date is committed, attached to a pull
request or linked from anywhere, and none will be. Every figure above
therefore stands on the command recorded for it in the table, which
reproduces it; where a command needs a source that is in no repository,
that source is printed in full beneath the table.
