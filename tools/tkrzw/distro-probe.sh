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
# A build that binds those references inside the library at link time — which
# is what `-Wl,-Bsymbolic-functions` does, and what Ubuntu's dpkg vendor
# profile adds to every package — breaks that agreement. The symptoms are
# silent: TreeDBM records the comparator as "custom" (type byte 255) and can
# never reopen the file, `Remove` stores the REMOVE sentinel as the record's
# value instead of deleting it, and `Rebuild` aborts with CANCELED_ERROR.
#
# Two checks, either of which is conclusive on its own:
#   static  — count the GOT relocations libtkrzw keeps for the comparators.
#             Zero means the library resolved them to its own copies, so no
#             client can ever match them.
#   dynamic — create a database with the system's own tkrzw_dbm_util and read
#             it back with the same binary.
#
# Exit codes: 0 = healthy, 1 = pointer-identity broken, 2 = tkrzw not found.

set -u
WORK=${WORK:-/tmp/tkrzw-probe}
rm -rf "$WORK" 2>/dev/null
mkdir -p "$WORK" || exit 2
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
