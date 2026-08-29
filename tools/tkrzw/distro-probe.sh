#!/bin/sh
# distro-probe.sh — does this system's tkrzw honour its own pointer-identity
# protocol?
#
# tkrzw identifies three things by the *address* of a symbol rather than by
# its value: the built-in key comparators (`tkrzw_dbm_tree.cc` maps
# `key_comparator_ == LexicalKeyComparator` to the on-disk type byte) and the
# `DBM::RecordProcessor::NOOP` / `REMOVE` sentinels (its header says to compare
# `your_value.data() == NOOP.data()`). Both rely on a client binary and
# libtkrzw agreeing on one canonical address per symbol, which ELF normally
# guarantees by routing the library's address-taking through the GOT.
#
# Two independent build flags break that agreement, and neither implies the
# other, so both are checked here:
#
#   -Wl,-Bsymbolic-functions  binds the library's function references to its
#     own copies at link time, so no client's comparator pointer ever matches.
#     TreeDBM then records the comparator as "custom" (type byte 255) and can
#     never reopen the file.
#   -flto  gives most LTO partitions their own copy of the NOOP/REMOVE backing
#     literal, so `value.data() == NOOP.data()` fails. `Remove` stores the
#     REMOVE sentinel as the record's value instead of deleting it.
#
# Ubuntu applies both to every package it builds; Debian applies neither; Arch
# applies LTO only. See docs/findings/tkrzw-distro-compat.md.
#
# Three checks, each conclusive on its own:
#   static  — count the GOT relocations libtkrzw keeps for the comparators.
#             Zero means the library resolved them to its own copies, so no
#             client can ever match them.
#   dynamic — create a database with the system's own tkrzw_dbm_util and read
#             it back with the same binary.
#   remove  — delete a record and check it is actually gone.
#
# Exit codes: 0 = healthy, 1 = pointer-identity broken, 2 = tkrzw not found.

set -u
WORK=$(mktemp -d "${TMPDIR:-/tmp}/tkrzw-probe.XXXXXX") || exit 2
# Clean the scratch dir on any exit; on a signal, exit non-zero so the
# EXIT trap runs the cleanup and the interrupt is not swallowed.
trap 'rm -rf "$WORK"' 0
trap 'exit 1' HUP INT TERM
cd "$WORK" || exit 2

distro=$(. /etc/os-release 2>/dev/null && echo "$PRETTY_NAME")
echo "distro : ${distro:-unknown}"

if ! command -v tkrzw_dbm_util >/dev/null 2>&1; then
  echo "tkrzw  : tkrzw_dbm_util not on PATH"
  exit 2
fi
echo "tkrzw  : $(tkrzw_build_util version 2>&1 | head -1)"

pkg=$( (dpkg-query -W -f='${binary:Package} ${Version} ' 'libtkrzw*' 'tkrzw*' 2>/dev/null \
        || pacman -Q tkrzw 2>/dev/null \
        || rpm -q tkrzw 2>/dev/null) | tr -s ' ')
[ -n "$pkg" ] && echo "package: $pkg"

rc=0

# ── static check ────────────────────────────────────────────────────
lib=$(ldd "$(command -v tkrzw_dbm_util)" 2>/dev/null \
        | sed -n 's/.*=> \(.*libtkrzw[^ ]*\).*/\1/p' | head -1)
if [ -n "$lib" ] && command -v readelf >/dev/null 2>&1; then
  n=$(readelf -rW "$lib" 2>/dev/null | grep -c KeyComparator)
  if [ "$n" -eq 0 ]; then
    echo "static : BROKEN  $lib keeps no GOT relocation for the key comparators,"
    echo "                 so its addresses can never match a client's"
    rc=1
  else
    echo "static : ok      $lib keeps $n GOT relocations for the key comparators"
  fi
else
  echo "static : skipped (no readelf, or libtkrzw not dynamically linked)"
fi

# ── dynamic check ───────────────────────────────────────────────────
for ext in tkh tkt tks; do
  db="rt.$ext"
  if ! out=$(tkrzw_dbm_util create "$db" 2>&1); then
    echo "$ext    : create failed :: $out"
    rc=1
    continue
  fi
  tkrzw_dbm_util set "$db" alpha bravo >/dev/null 2>&1
  got=$(tkrzw_dbm_util get "$db" alpha 2>&1)
  if [ "$got" = "bravo" ]; then
    echo "$ext    : ok      wrote and read back one record"
  else
    echo "$ext    : BROKEN  wrote a record, read back '$got'"
    rc=1
  fi
done

# The NOOP/REMOVE sentinel half, which no amount of comparator health implies:
# a deleted record must be gone, not present carrying the five-byte sentinel.
if tkrzw_dbm_util create rm.tkh >/dev/null 2>&1; then
  tkrzw_dbm_util set rm.tkh alpha one >/dev/null 2>&1
  tkrzw_dbm_util set rm.tkh bravo two >/dev/null 2>&1
  tkrzw_dbm_util remove rm.tkh alpha >/dev/null 2>&1
  if tkrzw_dbm_util list rm.tkh 2>/dev/null | cut -f1 | grep -qx alpha; then
    echo "remove : BROKEN  the removed record is still present, carrying the REMOVE sentinel"
    rc=1
  else
    echo "remove : ok      the removed record is gone"
  fi
fi

# The TreeDBM comparator byte lives at offset 53 of the "TDB" opaque metadata
# block that TreeDBM keeps inside its HashDBM container: 1 = LexicalKeyComparator
# (the default), 255 = "some comparator I could not name", which no later open
# can resolve.
if [ -f rt.tkt ] && command -v od >/dev/null 2>&1; then
  byte=$(od -An -v -tu1 -N 256 rt.tkt | tr -s ' ' '\n' | grep -v '^$' | awk '
    { b[NR-1] = $1 }
    END {
      for (i = 0; i + 56 < NR; i++)
        if (b[i] == 84 && b[i+1] == 68 && b[i+2] == 66) { print b[i+53]; exit }
    }')
  [ -n "$byte" ] && echo "tkt    : on-disk key_comparator type byte = $byte (1 = lexical, 255 = unnameable)"
fi

[ $rc -eq 0 ] && echo "RESULT : healthy" || echo "RESULT : pointer identity is broken on this build"
exit $rc
