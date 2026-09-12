#!/usr/bin/env python3
"""series.py — append a nightly snapshot to the series and read it back.

The other half of tools/perf-gate/snapshot.sh. Design and rationale:
docs/perf/ci-perf-size-gate-proposal-2026-09-09.md.

This is not a gate and it does not run on the PR path. It reports a trend and
draws attention to a large night-over-night move. Three rules shape it, and
each exists because the rejected per-PR proposal got it wrong:

1. **Record freely, threshold almost nothing.** A noisy number in a series a
   person reads costs nothing. A number that fails a build has to be
   defensible every single night, and one that is not gets muted — at which
   point it is worse than nothing.

2. **The series localizes WHEN, not WHY.** A history says which night a level
   moved. It cannot say on the numbers alone whether the toolchain, the runner
   image or our own commits moved it: a step and a large refactor look
   identical. So the environment travels with each sample, and when the
   environment differs from the predecessor's, a move is reported as
   *unattributable* and never flagged. Flagging it would be asserting a cause
   the data does not carry.

3. **Absence is not zero.** A missing instrument (no valgrind) or a missing
   reader (an artifact built without --features alloc-count) records `null`.
   A null is never compared, never averaged, and never treated as an
   improvement.

Usage:
    series.py --snapshot snap.json --series-dir dir/ [--append] [--summary out]

`--append` is what writes to the series, and the workflow passes it only on
the `schedule` event. A manual `workflow_dispatch` run measures and reports
but must not become the predecessor the next real night compares against.

Exit codes:
    0  reported (including: nothing to compare, or moves that are
       unattributable because the environment changed)
    1  a deterministic metric moved more than the attention threshold with
       the environment unchanged — a human should look
    2  the snapshot is malformed
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# Night-over-night move that earns a human's attention. Not a budget and not a
# limit: main changes every day, so a small daily delta is the normal state of
# a healthy series. These pick out a jump, not a trend.
ATTENTION = 0.01  # 1%

# Deterministic under a fixed environment: these may be flagged.
FLAGGABLE = ("section_sum", "stripped_size", "alloc_count_per_cycle",
             "alloc_bytes_per_cycle", "ir_oxpinyin_object")
# Recorded and reported, never flagged. RSS moves with the allocator, the
# kernel and the host; the series is a trend to read, not a trigger.
REPORT_ONLY = ("rss_init_kib", "rss_cycle_kib")

ALL_METRICS = FLAGGABLE + REPORT_ONLY


def valid(doc: object) -> bool:
    """Is this a snapshot we can compare, structurally?

    Valid JSON is not enough. A list, or a metric that is a string, gets past
    json.loads and then raises somewhere deep in the comparison — an uncaught
    traceback where the contract says either "treat it as missing" (a
    predecessor) or "exit 2" (this run's own snapshot). One validator serves
    both so the two answers cannot drift apart.
    """
    if not isinstance(doc, dict):
        return False
    metrics = doc.get("metrics")
    if not isinstance(metrics, dict):
        return False
    return all(v is None or (isinstance(v, (int, float)) and not isinstance(v, bool))
               for v in metrics.values())


def load(path: Path) -> dict:
    try:
        doc = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        print(f"malformed snapshot {path}: {exc}", file=sys.stderr)
        raise SystemExit(2)
    if not valid(doc):
        print(f"malformed snapshot {path}: not an object with numeric-or-null "
              f"metrics", file=sys.stderr)
        raise SystemExit(2)
    return doc


def previous(series_dir: Path, current_name: str) -> dict | None:
    """The most recent sample that is not the one we just took.

    A missing predecessor is normal, not an error: the cache is not durable,
    GitHub evicts unused entries, and the first run restores nothing.
    """
    samples = sorted(p for p in series_dir.glob("*.json") if p.name != current_name)
    if not samples:
        return None
    try:
        doc = json.loads(samples[-1].read_text())
    except (OSError, json.JSONDecodeError):
        # A corrupt predecessor is a missing predecessor. The series continues
        # from tonight rather than failing on yesterday's bad write.
        return None
    return doc if valid(doc) else None


def env_delta(a: dict, b: dict) -> list[str]:
    """Which environment fields differ. Empty means like-for-like."""
    ea, eb = a.get("environment", {}), b.get("environment", {})
    return sorted(k for k in set(ea) | set(eb) if ea.get(k) != eb.get(k))


def fmt(value: object) -> str:
    if value is None:
        return "—"
    if isinstance(value, int):
        return f"{value:,}"
    return str(value)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--snapshot", required=True, type=Path)
    ap.add_argument("--series-dir", required=True, type=Path)
    ap.add_argument("--append", action="store_true",
                    help="persist this sample; the workflow passes it only on schedule")
    ap.add_argument("--summary", type=Path, help="write a markdown report here")
    args = ap.parse_args()

    snap = load(args.snapshot)
    metrics = snap["metrics"]  # load() has already validated the shape

    args.series_dir.mkdir(parents=True, exist_ok=True)
    stamp = snap.get("captured_utc", "unknown").replace(":", "").replace("-", "")
    name = f"{stamp}-{snap.get('commit', 'nocommit')[:12]}.json"

    prev = previous(args.series_dir, name)

    lines = ["## Perf snapshot", ""]
    lines.append(f"commit `{snap.get('commit', '?')[:12]}` · "
                 f"captured {snap.get('captured_utc', '?')} · event `{snap.get('event', '?')}`")
    lines.append("")

    flagged: list[str] = []

    if prev is None:
        lines.append("No predecessor in the series — nothing to compare against. "
                     "This is normal on a first run or after a cache eviction; "
                     "the sample is recorded and tonight becomes the baseline "
                     "for tomorrow.")
        lines.append("")
        lines.append("| metric | value |")
        lines.append("|---|---:|")
        for key in ALL_METRICS:
            lines.append(f"| `{key}` | {fmt(metrics.get(key))} |")
    else:
        moved_env = env_delta(prev, snap)
        lines.append(f"Compared against `{prev.get('commit', '?')[:12]}` "
                     f"({prev.get('captured_utc', '?')}).")
        lines.append("")
        if moved_env:
            lines.append(
                f"**The environment changed** since that sample "
                f"({', '.join(f'`{k}`' for k in moved_env)}). Differences below "
                "are reported but **not attributed and not flagged**: a series "
                "localizes the night a level moved, it does not say whether the "
                "toolchain or the source moved it."
            )
            lines.append("")
        lines.append("| metric | previous | current | delta | |")
        lines.append("|---|---:|---:|---:|---|")
        for key in ALL_METRICS:
            now, before = metrics.get(key), prev.get("metrics", {}).get(key)
            note = ""
            delta = "—"
            if now is None or before is None:
                note = "not measured" if now is None else "no predecessor value"
            else:
                diff = now - before
                # A zero predecessor has no percentage. Treating it as 0% (the
                # first version did) silently exempts the one transition most
                # worth seeing: a metric that was genuinely zero — no
                # allocations on the steady path, say — becoming non-zero.
                from_zero = before == 0 and now != 0
                pct = (diff / before) if before else 0.0
                delta = f"{diff:+,} (—)" if before == 0 else f"{diff:+,} ({pct:+.2%})"
                if key in REPORT_ONLY:
                    note = "trend only"
                elif moved_env:
                    note = "unattributable"
                elif from_zero or abs(pct) > ATTENTION:
                    note = "**flagged**"
                    shown = "from zero" if from_zero else f"{pct:+.2%}"
                    flagged.append(f"{key}: {before:,} → {now:,} ({shown})")
            lines.append(f"| `{key}` | {fmt(before)} | {fmt(now)} | {delta} | {note} |")

    if args.append:
        (args.series_dir / name).write_text(json.dumps(snap, indent=2, sort_keys=True) + "\n")
        # Keep the series bounded; the artifacts are the durable copy.
        samples = sorted(args.series_dir.glob("*.json"))
        for stale in samples[:-30]:
            stale.unlink()
        lines += ["", f"Appended to the series as `{name}` "
                      f"({len(list(args.series_dir.glob('*.json')))} samples retained)."]
    else:
        lines += ["", "_Not appended: the series is written only on the nightly "
                      "schedule, so a manual run cannot become tomorrow's "
                      "predecessor._"]

    if flagged:
        lines += ["", "### Flagged", ""]
        lines += [f"- {line}" for line in flagged]
        lines += ["", "A flag is an invitation to look, not a verdict. It covers "
                      "everything merged since the previous sample, so narrowing "
                      "it to one change is a manual bisect."]

    report = "\n".join(lines) + "\n"
    print(report)
    if args.summary:
        args.summary.write_text(report)

    return 1 if flagged else 0


if __name__ == "__main__":
    sys.exit(main())
