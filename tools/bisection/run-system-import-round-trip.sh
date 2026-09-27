#!/usr/bin/env bash
# #599: byte-for-byte same-backend SYSTEM_FILE interchange, both directions.
# Usage: <pinyin|zhuyin> <pin-so> <ox-so> <system-data-dir>
set -euo pipefail
facade=${1:?pinyin or zhuyin}
pin=$(realpath "${2:?pin-so}")
ox=$(realpath "${3:?ox-so}")
data=$(realpath "${4:?system-data-dir}")
root=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
case $facade in
  pinyin) cc -std=gnu11 -Wall -Wextra -Werror -O2 "$root/import-semantics-diff.c" -ldl -o "$work/driver" ;;
  zhuyin) cc -std=gnu11 -Wall -Wextra -Werror -O2 "$root/zhuyin-import-diff.c" $(pkg-config --cflags --libs glib-2.0) -ldl -o "$work/driver" ;;
  *) exit 2 ;;
esac
run() {
  local profile=$1 so=$2 library=$3
  shift 3
  (cd "$profile" && LD_LIBRARY_PATH="$(dirname "$so")" "$work/driver" "$so" "$data" "$library" "$@")
}
for library in 1 2 3 4; do
  mkdir "$work/pin-$library" "$work/ox-$library"
  run "$work/pin-$library" "$pin" "$library" > "$work/pin.log"
  run "$work/ox-$library" "$ox" "$library" > "$work/ox.log"
  diff -u "$work/pin.log" "$work/ox.log"
  for name in gb_char gbk_char opengram merged; do
    cmp "$work/pin-$library/user/$name.dbin" "$work/ox-$library/user/$name.dbin"
  done
  grep '^reopened ' "$work/pin.log" > "$work/expected"
  cp "$work/pin-$library/user/gb_char.dbin" "$work/before-gb_char.dbin"
  cp "$work/pin-$library/user/gbk_char.dbin" "$work/before-gbk_char.dbin"
  cp "$work/pin-$library/user/opengram.dbin" "$work/before-opengram.dbin"
  cp "$work/pin-$library/user/merged.dbin" "$work/before-merged.dbin"
  run "$work/pin-$library" "$ox" "$library" rewrite > "$work/ox-reads-pin"
  for name in gb_char gbk_char opengram merged; do
    cmp "$work/before-$name.dbin" "$work/pin-$library/user/$name.dbin"
  done
  diff -u "$work/expected" "$work/ox-reads-pin"
  run "$work/pin-$library" "$pin" "$library" read > "$work/pin-reads-rewrite"
  diff -u "$work/expected" "$work/pin-reads-rewrite"
  run "$work/ox-$library" "$pin" "$library" read > "$work/pin-reads-ox"
  diff -u "$work/expected" "$work/pin-reads-ox"
  printf '%s library %s: IDENTICAL pin->ox, pin->ox->pin, ox->pin\n' "$facade" "$library"
done
