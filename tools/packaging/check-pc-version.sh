#!/usr/bin/env bash
# check-pc-version.sh — static CI gate: every declaration of the drop-in's
# libpinyin version agrees with the pin record.
#
# The pin-built libpinyin is not available in CI (store-backends.yml header),
# so tools/packaging/check-pc-metadata.sh — the full installed-tree diff —
# runs locally. This is the part CI can check without the pin: the version
# the pin record carries (tools/packaging/pc-version.sh) against each capi
# crate's [package.metadata.capi] header subdirectory, asset destinations
# and pkg_config version, and the pc templates' use of @VERSION@ (a literal
# version there would bypass the record). The capi build scripts enforce
# the manifest half at build time too; this fails before any build.
#
# Usage: tools/packaging/check-pc-version.sh
# Exits 0 when everything agrees; 1 listing each disagreement.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
want="$(./pc-version.sh)"
bad=0

for crate in oxpinyin-capi oxpinyin-zhuyin-capi; do
  manifest="$REPO_ROOT/crates/$crate/Cargo.toml"
  # Every "libpinyin-<version>" string outside comments.
  while IFS= read -r dir; do
    if [ "$dir" != "libpinyin-$want" ]; then
      echo "error: $manifest declares \"$dir\", want \"libpinyin-$want\"" >&2
      bad=1
    fi
  done < <(grep -v '^[[:space:]]*#' "$manifest" | grep -o '"libpinyin-[^"]*"' | tr -d '"')
  grep -v '^[[:space:]]*#' "$manifest" | grep -q "\"libpinyin-$want\"" \
    || { echo "error: $manifest declares no \"libpinyin-$want\" include subdirectory" >&2; bad=1; }
  got="$(awk '/^\[/{s=$0} s=="[package.metadata.capi.pkg_config]" && /^version[[:space:]]*=/{gsub(/.*=[[:space:]]*"|".*/,""); print}' "$manifest")"
  if [ "$got" != "$want" ]; then
    echo "error: $manifest [package.metadata.capi.pkg_config] version is \"$got\", want \"$want\"" >&2
    bad=1
  fi
done

for template in "$REPO_ROOT/crates/oxpinyin-capi/libpinyin.pc.in" \
                "$REPO_ROOT/crates/oxpinyin-zhuyin-capi/libzhuyin.pc.in"; do
  if grep -v '^#' "$template" | grep -Eq '[0-9]+\.[0-9]+\.[0-9]+'; then
    echo "error: $template carries a literal version; it must use @VERSION@" >&2
    bad=1
  fi
done

[ "$bad" -eq 0 ] || exit 1
echo "OK: every drop-in version declaration is $want (tools/oracle/oracle-pin.txt)"
