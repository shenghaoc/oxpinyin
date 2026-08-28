# Ubuntu's libtkrzw cannot read a database it writes

Date: 2026-08-28 · Status: **investigation finding** (no shipping code
changed; guidance and a probe added) · Branch:
`claude/tkrzw-distro-compat-9o3k8h`.

`oxpinyin-store`'s tkrzw backend already carried a warning that Ubuntu
noble's `libtkrzw-dev 1.0.27-1.1build1` breaks tkrzw's pointer-identity
protocol, and that `./configure && make` on the same sources is correct.
This note establishes *why*, and how far the breakage reaches.

The short version: the distro package is fine; **one linker flag that
Ubuntu adds to every package it builds** is not. Debian does not add it.

## What was measured

| System | tkrzw | `.tkh` | `.tkt` | `.tks` |
| --- | --- | --- | --- | --- |
| Ubuntu 24.04.4 (noble), `1.0.27-1.1build1` | 1.0.27 | ok | **broken** | ok |
| Ubuntu 26.04.1 (resolute), `1.0.32-1build1` | 1.0.32 | ok | **broken** | ok |
| upstream `1.0.27`, `./configure && make` | 1.0.27 | ok | ok | ok |
| Debian source pkg `1.0.27-1.1`, Debian build flags | 1.0.27 | ok | ok | ok |
| Debian source pkg `1.0.27-1.1`, Ubuntu build flags | 1.0.27 | ok | **broken** | ok |
| Debian source pkg `1.0.32-1`, Debian build flags | 1.0.32 | ok | ok | ok |
| Debian source pkg `1.0.32-1`, Ubuntu build flags | 1.0.32 | ok | **broken** | ok |

The last two rows are the version that matters for both open questions:
`1.0.32-1` is what Debian uploaded to unstable and what Ubuntu 26.04
rebuilt as `1.0.32-1build1`. Built with Ubuntu's flags it reproduces
26.04's shipped failure exactly; built with Debian's, from the same
tarball on the same machine, it is healthy — 18 GOT relocations kept
instead of none, comparator byte 1 instead of 255, and the library-API
probe below green on every row.

"broken" is literal: `tkrzw_dbm_util create x.tkt` succeeds, and every
subsequent open of that file by the same binary fails with
`BROKEN_DATA_ERROR: invalid_key_comparator`. HashDBM (`.tkh`) and SkipDBM
(`.tks`) are unaffected — neither stores a comparator.

Debian sid and Arch could not be run: this session's egress policy
returns 403 for every Debian and Arch mirror (`deb.debian.org`,
`geo.mirror.pkgbuild.com`, `fastly.mirror.pkgbuild.com`, and the rest —
inside a container too, and still 403 once the proxy CA is installed, so
it is policy and not TLS), and neither base image ships tkrzw. What
*can* be read offline is the one input that decides the outcome — each
distro's default `LDFLAGS` — and that is settled below.

## What decides it, per distro

The bisection further down reduces the whole question to one bit: does
this distro link shared libraries with `-Wl,-Bsymbolic-functions`?
Every distro's answer is readable from primary sources without
installing tkrzw.

| Distro | Where the default lives | `-Bsymbolic-functions`? |
| --- | --- | --- |
| Ubuntu | `Dpkg::Vendor::Ubuntu` line 156 | **yes** — `$flags->prepend('LDFLAGS', ...)` |
| Debian | `Dpkg::Vendor::Debian` | no — zero occurrences |
| Devuan, PureOS | their `Dpkg::Vendor::*` | no — zero occurrences |
| Arch | `/etc/makepkg.conf` (`pacman 7.1.0.r9`) | no |

The dpkg figures are from dpkg's own git at tag **1.23.7** — the exact
dpkg the `debian:sid-slim` image reports — so this is Debian's file, not
Ubuntu's copy of it. Ubuntu is the only vendor module of the four that
mentions the flag at all. Arch's full default is

```
-Wl,-O1 -Wl,--sort-common -Wl,--as-needed -Wl,-z,relro -Wl,-z,now -Wl,-z,pack-relative-relocs
```

and its `-fno-plt` in `CFLAGS` is not a hazard here: it changes how
calls are routed, not how addresses are taken.

A package can still add the flag itself, and tkrzw's does not. Debian's
`debian/rules` never sets `LDFLAGS` — the only two mentions are a
commented-out `DEB_LDFLAGS_MAINT_APPEND` — and the file is *byte
identical* from `1.0.27-1.1` through `1.0.32-1`, the version Debian
uploaded to unstable in October 2024 and the one Ubuntu 26.04 rebuilt as
`1.0.32-1build1`.

So Debian sid and Arch are unrun but not unknown: both feed the correct
input to the only step that matters. Arch carries the extra caveat that
this note never confirmed tkrzw is packaged for it at all — if it is
only in the AUR, `makepkg` still applies the same `/etc/makepkg.conf`.
"Finishing the matrix" below has the two commands that turn this from
determined into measured.

## The mechanism

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
one canonical definition. A correct build keeps 14 such relocations for
the comparators; a broken build keeps none.

`-Wl,-Bsymbolic-functions` is exactly the flag that removes them. It
binds a shared library's function references to that library's own
definitions at link time. `libtkrzw.so` then compares against its own
copy, `tkrzw_dbm_util` passes its own copy — and no built-in comparator
is ever recognised. `--comparator` defaults to `lex`
(`tkrzw_dbm_util.cc:630`), so *every* `.tkt` the CLI creates records 255.

Ubuntu adds that flag to every package it builds, in the dpkg vendor
profile itself — `/usr/share/perl5/Dpkg/Vendor/Ubuntu.pm:216`:

```perl
# Per https://wiki.ubuntu.com/DistCompilerFlags
$flags->prepend('LDFLAGS', '-Wl,-Bsymbolic-functions');
```

Debian's `Dpkg::Vendor::Debian` does not mention `-Bsymbolic` at all:

```
$ DEB_VENDOR=Debian dpkg-buildflags --get LDFLAGS
-Wl,-z,relro
$ DEB_VENDOR=Ubuntu dpkg-buildflags --get LDFLAGS
-Wl,-Bsymbolic-functions -flto=auto -ffat-lto-objects -Wl,-z,relro
```

So the packaging is not at fault, and neither is any Ubuntu delta to it.
Ubuntu's `1.0.27-1.1build1` is Debian's `1.0.27-1.1` plus one changelog
stanza that says so in as many words — "No-change rebuild for
CVE-2024-3094" — and `debian/rules` is nine lines that pass
`--enable-{zlib,zstd,lz4,lzma}`, set `hardening=+all`, skip the test
suite, and hand everything else to `dh`. Both builds below use that one
`.debian.tar.xz`; the only variable is `DEB_VENDOR`.

## The bisection

Building Debian's own `tkrzw_1.0.27-1.1` source package twice on one
machine, changing only `DEB_VENDOR`:

```
vendor=Debian  LDFLAGS=-Wl,-z,relro
  libGOTrelocs=14  libBind=WEAK    key_comp_type=1    get='world'

vendor=Ubuntu  LDFLAGS=-Wl,-Bsymbolic-functions -flto=auto -ffat-lto-objects -Wl,-z,relro
  libGOTrelocs=0   libBind=GLOBAL  key_comp_type=255  get='OpenAdvanced failed: BROKEN_DATA_ERROR: invalid_key_comparator'
```

The Ubuntu-vendor build reproduces the shipped package exactly, down to
the symbol binding. Narrowing to the single flag, twice — once on
upstream objects with no LTO, once on the Ubuntu-vendor objects with LTO
held on — relinking the same `.o` files each time:

```
A. upstream 1.0.27 objects, no LTO
   no extra LDFLAGS                  GOTrelocs=14  libBind=WEAK    key_comp_type=1
   + -Wl,-Bsymbolic-functions        GOTrelocs=0   libBind=WEAK    key_comp_type=255

B. Ubuntu-vendor objects, LTO on
   LTO, WITHOUT -Bsymbolic-functions GOTrelocs=14  libBind=GLOBAL  key_comp_type=1
   LTO, WITH    -Bsymbolic-functions GOTrelocs=0   libBind=GLOBAL  key_comp_type=255
```

`-Wl,-Bsymbolic-functions` is necessary and sufficient, in both
directions and with LTO on or off. LTO is a red herring: it turns the
comparators' COMDAT `WEAK` binding into `GLOBAL`, which is what makes
the shipped package look unusual under `nm`, but it does not change the
outcome on its own.

## It is not only the CLI

The same flag breaks all three of tkrzw's pointer-identity sites for any
*client* of the library, which is what makes it dangerous for a backend
like ours. A client compiled against each build
(`tools/tkrzw/identity-probe.cc`):

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

Row (a) is the one piece of good news, and it is why our backend has not
tripped over the comparator half: leaving `key_comparator` at its
`nullptr` default keeps the decision inside the library, where both
sides of the comparison are the same copy. `oxpinyin-store` installs no
comparator, so its files stay readable. Rows (c)–(e) it *is* exposed to,
and they are silent — a `Remove` that stores the sentinel as the value
looks like a successful write.

## What this means for us

Nothing in `oxpinyin` changes. The tkrzw backend is an evaluation
subject behind an off-by-default cargo feature, it already requires a
source build, and `build.rs` already refuses to proceed without one.
This note updates that guidance in two places: the warning named noble's
`1.0.27-1.1build1` specifically, and the fault is neither
version-specific nor noble-specific — every Ubuntu release ships it, and
26.04's `1.0.32-1build1` fails identically.

The one-line check, which needs no build and no test database:

```sh
readelf -rW /usr/lib/*/libtkrzw.so.1 | grep -c KeyComparator   # 0 = broken
```

`tools/tkrzw/distro-probe.sh` wraps that plus a write-then-read
round-trip, and exits non-zero on a broken build.

## Finishing the matrix

Debian sid and Arch stay unrun only because their mirrors are
unreachable from here. On a machine with normal egress:

```sh
docker run --rm -v "$PWD/tools/tkrzw/distro-probe.sh:/probe.sh:ro" debian:sid-slim \
  sh -c 'apt-get update -qq && apt-get install -y -qq tkrzw-utils binutils && sh /probe.sh'

docker run --rm -v "$PWD/tools/tkrzw/distro-probe.sh:/probe.sh:ro" archlinux:latest \
  sh -c 'pacman -Sy --noconfirm tkrzw binutils && sh /probe.sh'
```

Both are expected to print `RESULT : healthy`, on the evidence in "What
decides it, per distro": neither vendor adds the flag, tkrzw's packaging
does not either, and the flag is the whole of the fault. Should Debian
come back broken, the thing to look at is not the packaging but whether
its buildd flags have changed — `dpkg-buildflags --get LDFLAGS` inside
the container answers that in one line.

That expectation inverts the usual Debian-upstream-of-Ubuntu reasoning,
and deliberately so. Ubuntu's `1.0.27-1.1build1` is not a fork of
Debian's package but a rebuild of it that changed nothing but the
changelog; the defect enters at rebuild time, from a vendor-wide flag,
so it cannot have been inherited. A Debian fix would not reach Ubuntu
either — the flag would still be applied on top.

## Upstream

Upstream is unfixed as of `bcaa0fb`: the comparators are still `inline`
in a header and `tkrzw_dbm_tree.cc` still identifies them by pointer.
Identifying a value by an address that C++ only guarantees under
default ELF interposition is fragile — a distro flag, a static link of
one side, or `dlopen` with `RTLD_LOCAL` all break it, and each breaks it
silently and on disk. A one-byte enum in `TuningParameters`, or a name
string, would cost nothing and be immune. This is worth reporting
upstream together with the Ubuntu bug; it is not a Rust-mechanism
divergence, so it does not belong in `upstream-divergences.md`.
