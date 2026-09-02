#!/usr/bin/env bash
# Same-backend drop-in differential for oxpinyin-datagen.
#
# Compiles one model directory twice — with libpinyin's own build-time
# tools (gen_binary_files, import_interpolation, gen_unigram) and with
# oxpinyin-datagen — and compares the results through
# crates/oxpinyin-datagen/tests/libpinyin_parity.rs: every per-library
# chunk file byte-exact, every DBM row field-exact. Neither side reads the
# other's output (the canonical-source invariant).
#
# Runs inside the perf-matrix container
# (tools/bisection/Dockerfile.perf-matrix), which carries both libpinyin
# builds under /opt/libpinyin-{kc,tkrzw}.
#
#   docker run --rm --platform linux/arm64 -v "$PWD":/work -w /work \
#     -v "$PWD/target/model20/extracted":/model/extracted \
#     oxpinyin-matrix:latest \
#     tools/datagen/libpinyin-drop-in-differential.sh kc /model/extracted
#
#   # the toned mini model, both backends:
#   ... tools/datagen/libpinyin-drop-in-differential.sh kc fixtures/datagen-toned
#   ... tools/datagen/libpinyin-drop-in-differential.sh tkrzw fixtures/datagen-toned
#
# usage: libpinyin-drop-in-differential.sh <kc|tkrzw> <model-dir> [<work-dir>]
set -euo pipefail

backend=${1:?backend: kc or tkrzw}
model=$(cd "${2:?model dir}" && pwd)
work=${3:-/tmp/drop-in-$backend}

case $backend in
  kc)
    prefix=/opt/libpinyin-kc
    features=()
    datagen_backend=kyotocabinet
    ;;
  tkrzw)
    prefix=/opt/libpinyin-tkrzw
    features=(--no-default-features --features tkrzw)
    datagen_backend=tkrzw
    ;;
  *)
    echo "unknown backend $backend (kc or tkrzw)" >&2
    exit 2
    ;;
esac

theirs=$work/libpinyin
ours=$work/oxpinyin
rm -rf "$work"
mkdir -p "$theirs" "$ours"

# libpinyin's tools read table.conf from the working directory and every
# .table from --table-dir; they write every output into the working
# directory (SYSTEM_PINYIN_INDEX et al. are bare file names).
cp "$prefix/lib/libpinyin/data/table.conf" "$theirs/"
(
  cd "$theirs"
  "$prefix/bin/gen_binary_files" --gen-punct-table --table-dir "$model"
  "$prefix/bin/import_interpolation" --table-dir "$model" < "$model/interpolation2.text"
  "$prefix/bin/gen_unigram" --table-dir "$model"
)
echo "libpinyin ($backend) wrote:"
ls "$theirs"

# oxpinyin's producer: the normal CLI, the normal backend selection. The
# writer re-reads every file it wrote and refuses to report success unless
# every row reads back.
cargo run --locked -q -p oxpinyin-datagen "${features[@]}" -- compile \
  --backend "$datagen_backend" --model-dir "$model" --out-dir "$ours"
echo "oxpinyin ($backend) wrote:"
ls "$ours"

# The differential proper.
OXPINYIN_LIBPINYIN_DATA_DIR=$theirs PINYIN_MODEL_DIR=$model \
  cargo test --locked -p oxpinyin-datagen "${features[@]}" \
  --test libpinyin_parity -- --nocapture

# And the chunk files the CLI wrote, against libpinyin's, byte for byte
# (the test above compares the in-memory compile; this is the on-disk
# product of the command a packager runs).
for chunk in "$theirs"/*.bin; do
  name=$(basename "$chunk")
  case $name in
    pinyin_index.bin|phrase_index.bin|addon_pinyin_index.bin|addon_phrase_index.bin|punct.bin)
      continue ;;
  esac
  cmp "$chunk" "$ours/$name"
  echo "$name: byte-exact"
done
echo "drop-in differential ($backend, $model): OK"
