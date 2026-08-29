#!/bin/sh
# libpinyin-backend-probe.sh — which DBM backend does this system's libpinyin
# actually use?
#
# libpinyin selects one storage backend at configure time (`--with-dbm=` one of
# BerkeleyDB, KyotoCabinet, Tkrzw) and links the corresponding library into
# libpinyin.so. Reading that link is ground truth for the backend, independent
# of the build recipe. This is the command behind the per-distro backend matrix
# in docs/findings/tkrzw-distro-compat.md — distro-probe.sh inspects libtkrzw
# via tkrzw_dbm_util and never looks at libpinyin.so, so it cannot answer this.
#
# Exit codes: 0 = libpinyin found and inspected, 2 = libpinyin not found,
# 3 = could not inspect the ELF (no readelf/objdump, or it was unreadable).

set -u

. /etc/os-release 2>/dev/null
echo "distro  : ${PRETTY_NAME:-unknown}"

# Locate libpinyin.so: the ld.so cache first, then a filesystem sweep.
lib=$(ldconfig -p 2>/dev/null | sed -n 's/.*=> \(.*libpinyin\.so[^ ]*\).*/\1/p' | head -1)
[ -z "$lib" ] && lib=$(find /usr /lib /lib64 -name 'libpinyin.so*' 2>/dev/null | head -1)
if [ -z "$lib" ]; then
  echo "libpinyin: not found (package not installed)"
  exit 2
fi
echo "so      : $lib"

ver=$( (rpm -q --qf '%{VERSION}-%{RELEASE}\n' libpinyin 2>/dev/null; \
        dpkg-query -W -f='${Version}\n' 'libpinyin[0-9]*' 2>/dev/null; \
        pacman -Q libpinyin 2>/dev/null) | grep -v '^$' | head -1)
echo "version : ${ver:-?}"

# The backend is whichever storage library libpinyin.so links *directly*.
# Read its DT_NEEDED entries with readelf/objdump rather than ldd, whose
# transitive closure could surface a DBM library pulled in by some other
# dependency and misattribute the backend. A failed inspection exits
# non-zero — it is never silently classified as unknown.
if command -v readelf >/dev/null 2>&1; then
  needed=$(readelf -dW "$lib" 2>/dev/null) \
    || { echo "backend : unknown (readelf could not read $lib)"; exit 3; }
  needed=$(printf '%s\n' "$needed" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')
elif command -v objdump >/dev/null 2>&1; then
  needed=$(objdump -p "$lib" 2>/dev/null) \
    || { echo "backend : unknown (objdump could not read $lib)"; exit 3; }
  needed=$(printf '%s\n' "$needed" | awk '$1 == "NEEDED" { print $2 }')
else
  echo "backend : unknown (need readelf or objdump; install binutils)"
  exit 3
fi

hit=$(printf '%s\n' "$needed" | grep -iE 'libtkrzw|kyotocabinet|libdb-|libdb5|libdb\.so' | head -1)
case "$hit" in
  *tkrzw*)          backend="Tkrzw" ;;
  *kyotocabinet*)   backend="KyotoCabinet" ;;
  *libdb*)          backend="BerkeleyDB" ;;
  *)                backend="unknown (no known DBM library in DT_NEEDED)" ;;
esac
echo "backend : $backend"
[ -n "$hit" ] && echo "needed  : $hit"
