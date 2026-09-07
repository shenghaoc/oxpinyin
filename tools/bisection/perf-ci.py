#!/usr/bin/env python3
"""perf-ci.py — medians with percentile-bootstrap CIs over bisect --perf captures.

`bisect --perf` writes one JSON object per process; `run-perf-matrix.sh` and
`run-perf-same-data.sh` append them to a speed.jsonl. Their inline summarizers
print bare medians. Findings documents have quoted 95% CIs alongside those
medians since the 2026-09 matrices, but the script that produced them was never
committed, so the intervals in those documents are not reproducible from the
tree. This tool closes that gap going forward. It makes no attempt to reproduce
any previously published interval.

Resampling unit: the process. Cycles inside one process share a warm cache, a
page-table state and a CPU migration history, so they are not independent
draws; resampling them individually would understate the interval. Every
statistic here therefore resamples whole runs with replacement and recomputes
the statistic over the selected runs' samples ("whole-run resampling").

Metrics, matching the inline summarizers exactly:
    init    per process, init_ns
    alloc   per process, alloc_ns
    cold    per process, cycles_ns[0]
    steady  pooled cycles_ns[1:] of every process

Rows are grouped by (backend, repeats), so a workload sweep captured into one
file — or into one file per size, all passed at once — tabulates directly.

Deterministic: the resampler is seeded (--seed, default 20260907), so a rerun
over the same capture reproduces the same interval bit for bit.

Usage:
    perf-ci.py speed.jsonl [more.jsonl ...] [--metric steady] [--resamples N]
    perf-ci.py out-n*/speed.jsonl --format md > table.md
"""

import argparse
import json
import random
import statistics
import sys

METRICS = ("init", "alloc", "cold", "steady")


def load(paths):
    """Read capture files into {(backend, repeats): [per-process sample lists]}.

    Each process contributes one list per metric: a single-element list for the
    per-process metrics, and its cycles_ns[1:] for steady. Keeping the nesting
    is what makes whole-run resampling possible downstream.
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
                # `repeats` predates nothing: captures taken before the knob
                # existed have no such field and are, by construction, size 1.
                key = (row.get("backend", "?"), int(row.get("repeats", 1)))
                cycles = row["cycles_ns"]
                if not cycles:
                    continue
                per_run = cells.setdefault(key, [])
                per_run.append({
                    "init": [row["init_ns"]],
                    "alloc": [row["alloc_ns"]],
                    "cold": [cycles[0]],
                    "steady": list(cycles[1:]),
                })
    return cells


def pooled(runs, metric):
    return [v for run in runs for v in run[metric]]


def bootstrap_ci(runs, metric, resamples, seed, alpha=0.05):
    """Percentile bootstrap of the median, resampling whole runs."""
    observed = pooled(runs, metric)
    if not observed:
        return float("nan"), float("nan"), float("nan"), 0
    point = statistics.median(observed)
    n = len(runs)
    if n < 2:
        return point, float("nan"), float("nan"), len(observed)

    rng = random.Random(seed)
    medians = []
    for _ in range(resamples):
        picked = [runs[rng.randrange(n)] for _ in range(n)]
        samples = pooled(picked, metric)
        if samples:
            medians.append(statistics.median(samples))
    if not medians:
        return point, float("nan"), float("nan"), len(observed)
    medians.sort()
    lo = medians[max(0, int(round((alpha / 2) * (len(medians) - 1))))]
    hi = medians[min(len(medians) - 1,
                     int(round((1 - alpha / 2) * (len(medians) - 1))))]
    return point, lo, hi, len(observed)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("captures", nargs="+",
                        help="speed.jsonl files from bisect --perf")
    parser.add_argument("--metric", action="append", choices=METRICS,
                        help="metric to report (repeatable; default: all)")
    parser.add_argument("--resamples", type=int, default=10000,
                        help="bootstrap resamples (default 10000)")
    parser.add_argument("--seed", type=int, default=20260907,
                        help="resampler seed; fixed so intervals reproduce")
    parser.add_argument("--format", choices=("text", "md"), default="text")
    args = parser.parse_args(argv)

    if args.resamples < 1:
        sys.exit("--resamples must be >= 1")
    metrics = args.metric or list(METRICS)

    cells = load(args.captures)
    if not cells:
        sys.exit("no cycle-bearing rows in the given captures")

    header = ["cell", "n", "runs", "samples"] + [f"{m} ms [95% CI]" for m in metrics]
    rows = []
    for (backend, repeats) in sorted(cells):
        runs = cells[(backend, repeats)]
        cells_out = [backend, str(repeats), str(len(runs))]
        counts = set()
        for metric in metrics:
            point, lo, hi, count = bootstrap_ci(
                runs, metric, args.resamples, args.seed)
            counts.add(count)
            cells_out.append(f"{point / 1e6:.3f} [{lo / 1e6:.3f}, {hi / 1e6:.3f}]")
        # Sample counts differ between per-process and pooled metrics; report
        # the largest, which is the pooled steady count when steady is asked
        # for and the run count otherwise.
        rows.append(cells_out[:3] + [str(max(counts))] + cells_out[3:])

    if args.format == "md":
        print("| " + " | ".join(header) + " |")
        print("|" + "|".join("---" for _ in header) + "|")
        for row in rows:
            print("| " + " | ".join(row) + " |")
    else:
        widths = [max(len(header[i]), *(len(r[i]) for r in rows))
                  for i in range(len(header))]
        print("  ".join(h.ljust(w) for h, w in zip(header, widths)).rstrip())
        for row in rows:
            print("  ".join(c.ljust(w) for c, w in zip(row, widths)).rstrip())
    print(f"\n# whole-run percentile bootstrap; {args.resamples} resamples; "
          f"seed {args.seed}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
