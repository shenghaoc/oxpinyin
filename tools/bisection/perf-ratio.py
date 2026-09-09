#!/usr/bin/env python3
"""perf-ratio.py — whole-run bootstrap CI for the ratio of two cells' medians.

`perf-ci.py` bootstraps each cell's median independently and prints one
interval per cell. The steady-cycle decision rule reads the RATIO
ox ÷ lp, and an interval for a ratio cannot be assembled from two
independent intervals for its terms: it needs paired resampling. This tool
draws whole runs with replacement from each cell — the same resampling
unit, resample count and seeding discipline as `perf-ci.py` — recomputes
median(numerator) / median(denominator) on each resample, and reports the
percentiles of that distribution as the interval the decision rule reads.

It lives in the tree rather than in a session's scratch helper because it
produces the most load-bearing number of a capture: the amd64 session's
ratio intervals ([0.9136, 0.9846] et al.) came from exactly such a helper
that was never committed, so those intervals are not reproducible from any
script the tree retains. Every capture from here on ratios through this.

Deterministic: the resampler is seeded (--seed, default 20260907,
`perf-ci.py`'s value), so a rerun over the same capture reproduces the
same interval bit for bit.

Usage:
    perf-ratio.py speed.jsonl --numerator oxpinyin-tkrzw \
        --denominator libpinyin-tkrzw
    perf-ratio.py out-n*/speed.jsonl --numerator oxpinyin-nodebug \
        --denominator libpinyin-tkrzw --repeats 8
"""

import argparse
import json
import random
import statistics
import sys

METRICS = ("init", "alloc", "cold", "steady")


def load(paths, wanted, repeats):
    """Read capture files into {backend: [per-process sample lists]}.

    Only the two requested backends at the one requested repeats level are
    kept, and the per-run nesting is preserved so whole-run resampling
    stays possible — the same loader discipline as `perf-ci.py`.
    """
    cells = {}
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
                    continue  # ram-init rows carry no cycle timings
                backend = row.get("backend", "?")
                if backend not in wanted:
                    continue
                if int(row.get("repeats", 1)) != repeats:
                    continue
                cycles = row["cycles_ns"]
                if not cycles:
                    continue
                cells.setdefault(backend, []).append({
                    "init": [row["init_ns"]],
                    "alloc": [row["alloc_ns"]],
                    "cold": [cycles[0]],
                    "steady": list(cycles[1:]),
                })
    return cells


def pooled(runs, metric):
    return [v for run in runs for v in run[metric]]


def bootstrap_ratio(num_runs, den_runs, metric, resamples, seed, alpha=0.05):
    """Percentile bootstrap of median(num)/median(den), resampling whole
    runs independently within each cell. Cycles inside one process share a
    warm cache, a page-table state and a CPU migration history, so they are
    not independent draws (`perf-ci.py`'s argument, verbatim); the process
    is the resampling unit for both terms."""
    num = pooled(num_runs, metric)
    den = pooled(den_runs, metric)
    if not num or not den:
        sys.exit(f"metric {metric}: a cell has no samples")
    if len(num_runs) < 2 or len(den_runs) < 2:
        sys.exit("each cell needs >= 2 runs to resample")
    med_num = statistics.median(num)
    med_den = statistics.median(den)
    point = med_num / med_den

    rng = random.Random(seed)
    ratios = []
    for _ in range(resamples):
        picked_num = [num_runs[rng.randrange(len(num_runs))]
                      for _ in range(len(num_runs))]
        picked_den = [den_runs[rng.randrange(len(den_runs))]
                      for _ in range(len(den_runs))]
        s_num = pooled(picked_num, metric)
        s_den = pooled(picked_den, metric)
        if s_num and s_den:
            ratios.append(statistics.median(s_num) / statistics.median(s_den))
    if not ratios:
        sys.exit("no valid resamples")
    ratios.sort()
    lo = ratios[max(0, int(round((alpha / 2) * (len(ratios) - 1))))]
    hi = ratios[min(len(ratios) - 1,
                    int(round((1 - alpha / 2) * (len(ratios) - 1))))]
    return med_num, med_den, point, lo, hi


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("captures", nargs="+",
                        help="speed.jsonl files from bisect --perf")
    parser.add_argument("--numerator", required=True,
                        help="backend label of the ratio's numerator cell")
    parser.add_argument("--denominator", required=True,
                        help="backend label of the ratio's denominator cell")
    parser.add_argument("--metric", choices=METRICS, default="steady",
                        help="metric to ratio (default: steady)")
    parser.add_argument("--repeats", type=int, default=1,
                        help="workload size to select (default: 1)")
    parser.add_argument("--resamples", type=int, default=10000,
                        help="bootstrap resamples (default 10000)")
    parser.add_argument("--seed", type=int, default=20260907,
                        help="resampler seed; fixed so intervals reproduce")
    parser.add_argument("--format", choices=("text", "md"), default="text")
    args = parser.parse_args(argv)

    if args.resamples < 1:
        sys.exit("--resamples must be >= 1")
    wanted = (args.numerator, args.denominator)
    cells = load(args.captures, wanted, args.repeats)
    missing = [c for c in wanted if c not in cells]
    if missing:
        sys.exit(f"no cycle-bearing rows for: {', '.join(missing)} "
                 f"(repeats={args.repeats})")

    med_num, med_den, point, lo, hi = bootstrap_ratio(
        cells[args.numerator], cells[args.denominator],
        args.metric, args.resamples, args.seed)

    n_num = len(cells[args.numerator])
    n_den = len(cells[args.denominator])
    if args.format == "md":
        print("| numerator | denominator | metric | repeats | "
              "median num (ms) | median den (ms) | ratio | 95% CI | width |")
        print("|---|---|---|---|---:|---:|---:|---|---:|")
        print(f"| {args.numerator} | {args.denominator} | {args.metric} "
              f"| {args.repeats} | {med_num / 1e6:.3f} | {med_den / 1e6:.3f} "
              f"| {point:.4f} | [{lo:.4f}, {hi:.4f}] | {hi - lo:.4f} |")
    else:
        print(f"metric: {args.metric}  repeats: {args.repeats}")
        print(f"{args.denominator}: median {med_den / 1e6:.3f} ms "
              f"({n_den} runs)")
        print(f"{args.numerator}: median {med_num / 1e6:.3f} ms "
              f"({n_num} runs)")
        print(f"ratio {args.numerator} / {args.denominator}: {point:.4f}")
        print(f"95% CI [{lo:.4f}, {hi:.4f}] (width {hi - lo:.4f})")
    print(f"\n# whole-run percentile bootstrap; {args.resamples} resamples; "
          f"seed {args.seed}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
