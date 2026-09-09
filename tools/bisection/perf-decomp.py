#!/usr/bin/env python3
"""perf-decomp.py — within-session tree/recipe decomposition of a ratio shift.

The control (run-tree-recipe-control.sh) times four cells round-robin in one
session at one workload size:

    L  libpinyin (in-image)
    X  old tree, record recipe   (cargo build + strip)
    Y  current tree, record recipe
    Z  current tree, cargo cinstall

The steady ratio is ox/lp. Two effects are wanted, each holding one variable
constant:

    tree effect   = Y/L - X/L     (recipe fixed: X and Y both cargo build)
    recipe effect = Z/L - Y/L     (tree fixed: Y and Z both current)

Both are differences of ratios that share the L denominator, so the session
offset cancels in each difference; and because all four cells ran in the same
rounds, the pairing is real — round i of X, Y, Z and L were measured seconds
apart under identical co-tenant conditions.

Resampling is therefore over ROUNDS, not cells: a resample draws round indices
with replacement and recomputes every cell's median from the SAME drawn
rounds, so the difference of ratios is computed on paired data. Resampling
cells independently would break the pairing and inflate the interval of the
difference.

Seeding matches perf-ci.py / perf-ratio.py (default 20260907) so every number
in a capture reproduces bit for bit.

Usage:
    perf-decomp.py speed.jsonl --repeats 8 \
        --l libpinyin-tkrzw --x oxpinyin-old-cargo \
        --y oxpinyin-cur-cargo --z oxpinyin-cur-cinstall
"""

import argparse
import json
import random
import statistics
import sys


def load_rounds(paths, backends, repeats):
    """{backend: [per-round sample list]} — the i-th entry is round i.

    The control driver appends one row per cell per round in a fixed
    round-robin order, so the i-th row of each backend is the same round.
    Round alignment is asserted: every backend must contribute the same
    number of rows.
    """
    cells = {b: [] for b in backends}
    for path in paths:
        with open(path, encoding="utf-8") as handle:
            for lineno, line in enumerate(handle, 1):
                line = line.strip()
                if not line:
                    continue
                try:
                    row = json.loads(line)
                except json.JSONDecodeError as exc:
                    sys.exit(f"{path}:{lineno}: not JSON: {exc}")
                if "cycles_ns" not in row:
                    continue
                backend = row.get("backend", "?")
                if backend not in cells:
                    continue
                if int(row.get("repeats", 1)) != repeats:
                    continue
                cycles = row["cycles_ns"]
                if len(cycles) < 2:
                    continue
                cells[backend].append(list(cycles[1:]))  # steady only
    counts = {b: len(v) for b, v in cells.items()}
    if len(set(counts.values())) != 1:
        sys.exit(f"round counts are not aligned across cells: {counts}")
    if counts[backends[0]] < 2:
        sys.exit(f"need >= 2 rounds to resample; have {counts[backends[0]]}")
    return cells


def ratio_from(cells, rounds, num, den):
    """median(num) / median(den) over the given round indices."""
    n = [v for r in rounds for v in cells[num][r]]
    d = [v for r in rounds for v in cells[den][r]]
    return statistics.median(n) / statistics.median(d)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("captures", nargs="+")
    parser.add_argument("--l", required=True, help="denominator cell label")
    parser.add_argument("--x", required=True)
    parser.add_argument("--y", required=True)
    parser.add_argument("--z", required=True)
    parser.add_argument("--repeats", type=int, default=1)
    parser.add_argument("--resamples", type=int, default=10000)
    parser.add_argument("--seed", type=int, default=20260907)
    parser.add_argument("--format", choices=("text", "md"), default="text")
    args = parser.parse_args(argv)

    backends = [args.l, args.x, args.y, args.z]
    cells = load_rounds(args.captures, backends, args.repeats)
    n_rounds = len(cells[args.l])
    all_rounds = list(range(n_rounds))

    # Point estimates, in milliseconds and as ratios.
    def med(backend):
        return statistics.median([v for r in cells[backend] for v in r]) / 1e6

    r_x = ratio_from(cells, all_rounds, args.x, args.l)
    r_y = ratio_from(cells, all_rounds, args.y, args.l)
    r_z = ratio_from(cells, all_rounds, args.z, args.l)
    tree = r_y - r_x
    recipe = r_z - r_y

    # Paired bootstrap over rounds.
    rng = random.Random(args.seed)
    trees, recipes, xs, ys, zs = [], [], [], [], []
    for _ in range(args.resamples):
        drawn = [rng.randrange(n_rounds) for _ in range(n_rounds)]
        bx = ratio_from(cells, drawn, args.x, args.l)
        by = ratio_from(cells, drawn, args.y, args.l)
        bz = ratio_from(cells, drawn, args.z, args.l)
        xs.append(bx); ys.append(by); zs.append(bz)
        trees.append(by - bx)
        recipes.append(bz - by)

    def ci(values):
        values = sorted(values)
        lo = values[max(0, int(round(0.025 * (len(values) - 1))))]
        hi = values[min(len(values) - 1, int(round(0.975 * (len(values) - 1))))]
        return lo, hi

    out = []
    if args.format == "md":
        out.append("| cell | median steady ms | ratio vs L | 95% CI |")
        out.append("|---|---:|---:|---|")
        for label, r, bs in ((args.x, r_x, xs), (args.y, r_y, ys),
                             (args.z, r_z, zs)):
            lo, hi = ci(bs)
            out.append(f"| {label} | {med(label):.3f} | {r:.4f} | "
                       f"[{lo:.4f}, {hi:.4f}] |")
        out.append("")
        tlo, thi = ci(trees)
        rlo, rhi = ci(recipes)
        out.append("| effect | estimate | 95% CI (paired, over rounds) |")
        out.append("|---|---:|---|")
        out.append(f"| tree  = Y/L − X/L | {tree:+.4f} | "
                   f"[{tlo:+.4f}, {thi:+.4f}] |")
        out.append(f"| recipe = Z/L − Y/L | {recipe:+.4f} | "
                   f"[{rlo:+.4f}, {rhi:+.4f}] |")
    else:
        out.append(f"n_rounds={n_rounds}")
        for label, r, bs in ((args.x, r_x, xs), (args.y, r_y, ys),
                             (args.z, r_z, zs)):
            lo, hi = ci(bs)
            out.append(f"{label}: {med(label):.3f} ms  ratio {r:.4f} [{lo:.4f}, {hi:.4f}]")
        tlo, thi = ci(trees); rlo, rhi = ci(recipes)
        out.append(f"tree   = Y/L - X/L = {tree:+.4f} [{tlo:+.4f}, {thi:+.4f}]")
        out.append(f"recipe = Z/L - Y/L = {recipe:+.4f} [{rlo:+.4f}, {rhi:+.4f}]")

    print("\n".join(out))
    print(f"\n# paired round bootstrap; {args.resamples} resamples; "
          f"seed {args.seed}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
