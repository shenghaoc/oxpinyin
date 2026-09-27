#!/usr/bin/env bash
# oracle-cell.sh — which backend cell a differential runs on. Sourced,
# never executed.
#
# A cell is one DBM backend on both sides: the pin-built oracle
# configured `--with-dbm=<X>` and oxpinyin built with the matching cargo
# feature, reading data written by that same backend. The parity oracle
# defaults to Berkeley DB — the reference build is a bare `./configure`
# (human ruling 2026-09-27 UTC, following Q1; `configure.ac:94` at
# 074a2219) — and the tkrzw and kc cells stay selectable:
#
#   PINYIN_ORACLE_DBM=bdb|kc|tkrzw   (default bdb; the same names as
#                                     build-oracle.sh --dbm)
#
# After sourcing, the caller has:
#
#   ORACLE_DBM                 bdb | kc | tkrzw
#   ORACLE_DBM_NAME            BerkeleyDB | KyotoCabinet | Tkrzw
#   CAPI_FEATURE               bdb | kyotocabinet | tkrzw
#   OXPINYIN_CAPI_BACKEND_EXT  db | kct | tkt (exported, for system-dir.sh)
#   EXPECTED_PIN_REF           pin_ref=libpinyin-<ver>-<sha>+model20-<sha>+dbm-<cell>
#
# The pin constants are read from tools/oracle/build-oracle.sh, the one
# place they live, so a pin move cannot leave a runner's expected ref
# behind.
#
# REPO_ROOT must be set before sourcing.

ORACLE_DBM=${PINYIN_ORACLE_DBM:-bdb}
case $ORACLE_DBM in
bdb)
	ORACLE_DBM_NAME=BerkeleyDB
	CAPI_FEATURE=bdb
	OXPINYIN_CAPI_BACKEND_EXT=db
	;;
kc)
	ORACLE_DBM_NAME=KyotoCabinet
	CAPI_FEATURE=kyotocabinet
	OXPINYIN_CAPI_BACKEND_EXT=kct
	;;
tkrzw)
	ORACLE_DBM_NAME=Tkrzw
	CAPI_FEATURE=tkrzw
	OXPINYIN_CAPI_BACKEND_EXT=tkt
	;;
*)
	printf 'fatal: PINYIN_ORACLE_DBM=%q is not one of: bdb kc tkrzw\n' "$ORACLE_DBM" >&2
	exit 3
	;;
esac
export OXPINYIN_CAPI_BACKEND_EXT

oracle_cell_build_oracle=$REPO_ROOT/tools/oracle/build-oracle.sh
oracle_cell_version=$(sed -n 's/^LIBPINYIN_VERSION=//p' "$oracle_cell_build_oracle")
oracle_cell_sha=$(sed -n 's/^LIBPINYIN_SHA=//p' "$oracle_cell_build_oracle")
oracle_cell_model=$(sed -n 's/^MODEL_SHA256=//p' "$oracle_cell_build_oracle")
if [[ -z $oracle_cell_version || -z $oracle_cell_sha || -z $oracle_cell_model ]]; then
	printf 'fatal: could not read the pin constants from %s\n' "$oracle_cell_build_oracle" >&2
	exit 1
fi
EXPECTED_PIN_REF="pin_ref=libpinyin-$oracle_cell_version-$oracle_cell_sha+model20-$oracle_cell_model+dbm-$ORACLE_DBM"
