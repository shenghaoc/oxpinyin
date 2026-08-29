# Debian now ships libpinyin on tkrzw; Ubuntu's tkrzw silently corrupts records

Date: 2026-08-28 (cross-distro matrix measured 2026-08-29) · Status:
**investigation finding** (no shipping code changed; guidance and two
probes added) · Branch: `claude/tkrzw-distro-compat-9o3k8h`.

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
