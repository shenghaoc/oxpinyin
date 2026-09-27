#!/usr/bin/env bash
# #599: instruction counts (including phase breakdown), peak heap, and mapped
# pages for the same import/save workload. Output stays outside the repository.
# Usage: <pin-so> <ox-so> <system-data-dir> <new-output-dir> [iterations]
set -euo pipefail
pin=$(realpath "${1:?pin-so}")
ox=$(realpath "${2:?ox-so}")
data=$(realpath "${3:?system-data-dir}")
out=${4:?new-output-dir}
iterations=${5:-1000}
mkdir "$out"
out=$(realpath "$out")
root=$(cd "$(dirname "$0")" && pwd)
cc -std=gnu11 -Wall -Wextra -Werror -O2 "$root/system-import-profile.c" -ldl -o "$out/driver"
for side in pin ox; do
  so=$pin
  [[ $side == ox ]] && so=$ox
  for mode in callgrind heap pages; do
    options=(--tool=massif --massif-out-file="$out/$side-$mode.out")
    if [[ $mode == callgrind ]]; then
      options=(--tool=callgrind --callgrind-out-file="$out/$side-$mode.out")
    elif [[ $mode == pages ]]; then
      options+=(--pages-as-heap=yes)
    fi
    valgrind "${options[@]}" "$out/driver" "$so" "$data" "$out/$side-$mode-user" "$iterations" > "$out/$side-$mode.log" 2>&1
  done
done
python3 - "$out" <<'PY'
from pathlib import Path
import sys
root = Path(sys.argv[1])
for side in ('pin', 'ox'):
    total = 0
    for suffix, phase in (('.1', 'open'), ('.2', 'import'), ('.3', 'save'), ('', 'close')):
        lines = (root / f'{side}-callgrind.out{suffix}').read_text().splitlines()
        count = int(next(line.split(':')[1] for line in lines if line.startswith('summary:')))
        total += count
        print(f'{side} {phase}: {count} instructions')
    print(f'{side} total: {total} instructions')
    for mode in ('heap', 'pages'):
        lines = (root / f'{side}-{mode}.out').read_text().splitlines()
        peak = max(int(line.split('=')[1]) for line in lines if line.startswith('mem_heap_B='))
        print(f'{side} peak {mode}: {peak} bytes')
PY
