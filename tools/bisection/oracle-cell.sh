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

# Retained, cell-specific targets. Relative overrides are repository-relative,
# consistently with the original surface runners (which cd before sourcing).
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-${OXPINYIN_TARGET_ROOT:-$REPO_ROOT/target/cells}/$ORACLE_DBM}
case $CARGO_TARGET_DIR in
    /*) ;;
    *) CARGO_TARGET_DIR=$REPO_ROOT/$CARGO_TARGET_DIR ;;
esac
export CARGO_TARGET_DIR
export CARGO_PROFILE_DEV_OPT_LEVEL=1
export ORACLE_DBM ORACLE_DBM_NAME CAPI_FEATURE EXPECTED_PIN_REF

# Resolve/build only the requested artifact. Explicit prebuilt paths never
# invoke Cargo, and are checked before a driver can consume them.
oracle_cell_artifact() {
    local variable=$1 filename=$2 package=$3 artifact
    artifact=${!variable:-}
    if [[ -z $artifact ]]; then
        cargo build --locked --manifest-path "$REPO_ROOT/Cargo.toml" \
            -p "$package" --no-default-features --features "$CAPI_FEATURE" >&2 || return 1
        artifact=$CARGO_TARGET_DIR/debug/$filename
    else
        case $artifact in
            /*) ;;
            *) artifact=$REPO_ROOT/$artifact ;;
        esac
    fi
    if [[ ! -f $artifact ]]; then
        printf 'FAIL: missing %s artifact: %s\n' "$variable" "$artifact" >&2
        return 1
    fi
    printf -v "$variable" '%s' "$artifact"
    export "${variable?}"
}

# Resolve a registry-selected oracle variant beside the unpatched cell prefix.
# Keep this separate from subject artifact resolution: a runner's oracle patch
# must never change the cell's Cargo target or another runner's oracle.
oracle_cell_resolve_prefix() {
    local base=$1 variant=${2:-unpatched} patch_digest
    PINYIN_ORACLE_PREFIX=$base
    EXPECTED_ORACLE_PIN_REF=$EXPECTED_PIN_REF
    ORACLE_PATCH_FILE=
    case $variant in
        unpatched) ;;
        bigram-export-strjoinv)
            PINYIN_ORACLE_PREFIX=$base-$variant
            ORACLE_PATCH_FILE=pin-bigram-export-strjoinv.patch
            patch_digest=$(cd "$REPO_ROOT/tools/bisection/patches/$variant" &&
                find . -maxdepth 1 -type f -name '*.patch' -print0 |
                sort -z | xargs -0 sha256sum | sha256sum) || return 1
            EXPECTED_ORACLE_PIN_REF+=+patches-${patch_digest%% *}
            ;;
        *) printf 'FAIL: unknown oracle variant: %s\n' "$variant" >&2; return 1 ;;
    esac
    export PINYIN_ORACLE_PREFIX EXPECTED_ORACLE_PIN_REF ORACLE_PATCH_FILE
}
