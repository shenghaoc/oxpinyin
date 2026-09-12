#!/usr/bin/env python3
"""check.py — the perf-and-size gate's decision logic, as a standalone tool.

Implements the rules specified in
`docs/perf/ci-perf-size-gate-proposal-2026-09-09.md`. That document is the
authority; this file is its executable form, and where the two disagree the
document wins and this file is the bug.

**Nothing in `.github/` runs this yet.** The lane that would is a CI-policy
change, which AGENTS.md puts behind an explicit human ask, and that ask is
open. The tool exists so Phase 0 can be run by hand — capture, check, read the
exit code — without anybody having to approve a workflow first.

Usage:
    check.py --baseline perf/ci-baseline.json --capture out/capture.json
    check.py --baseline … --capture … --nightly     # also gates G4

Exits (the document's table, and the reason it has four and not two):
    0  every gated value within its threshold
    1  REGRESSION       — a gated value exceeded its limit
    2  INSTRUMENT FAULT — the measurement is not trustworthy
    3  BASELINE STALE   — the environment moved, or the baseline is malformed

Exit 1 and exit 2 are deliberately distinct. A gate whose flakes and whose real
findings look alike gets muted, and a muted gate is worse than no gate: it
reports green while nobody reads it.

The order of the checks is itself a rule, not an implementation detail:

    validity  →  agreement  →  comparison

A `-1` from one of the `oxpinyin_alloc_*` readers means `dlsym` failed and the
artifact was built without `--features alloc-count`; `bisect.c`'s
`read_alloc_counters` substitutes `-1` rather than failing. Two rounds of that
agree *exactly*, and `-1 - (-1) == 0` reads as zero allocations per cycle — an
apparent improvement that would ratchet the baseline to a number no real build
can ever meet. So validity is established before agreement is evaluated, and
agreement before anything is compared to the baseline.
"""

from __future__ import annotations

import argparse
import json
import re
import statistics
import sys
from pathlib import Path

EXIT_OK = 0
EXIT_REGRESSION = 1
EXIT_FAULT = 2
EXIT_STALE = 3

# Round 1 is the compared value for G1-G3; round 2 exists to falsify it, not to
# be averaged with it. G4 is the one statistical metric, so it takes the median
# of its passes.
AGREEMENT = {
    "g1": ("exact", 0.0),
    "g2": ("relative", 0.0005),  # 0.05%, floor measured at 0.031%
    "g3": ("exact", 0.0),
    "g4": ("relative", 0.01),  # 1% between two 10-process passes
}

# The one placeholder a recipe may legitimately carry: the per-run staging
# directory is a mktemp path with no bearing on the artifact, and recording it
# verbatim would change the string every run and make the fingerprint unequal
# to itself.
STAGE_TOKEN = "<stage>"
PLACEHOLDER_RE = re.compile(r"<[^>]*>|\.\.\.|[…]")


class Fault(Exception):
    """An instrument fault: the measurement cannot be trusted (exit 2)."""


class Stale(Exception):
    """The baseline does not describe this environment (exit 3)."""


def load(path: Path) -> dict:
    """Read a strict-JSON document, or fail as stale rather than crashing.

    The committed baseline is strict JSON by specification — no comments, no
    trailing commas — precisely so a hand-added annotation cannot take the lane
    down with a traceback. A malformed baseline is a stale baseline: it needs
    regenerating, which is the exit-3 remedy.
    """
    try:
        return json.loads(path.read_text())
    except FileNotFoundError as exc:
        raise Stale(f"{path}: not found") from exc
    except json.JSONDecodeError as exc:
        raise Stale(f"{path}: not strict JSON ({exc})") from exc


def check_recipes(doc: dict, where: str) -> None:
    """Every artifact names the exact command line that produced it.

    `docs/runbooks/benches.md` ("State the build recipe, per artifact")
    requires the command line, sha256, NEEDED list and byte size for every
    timed artifact. A baseline whose provenance field has been hand-edited into
    a fragment is the failure this block exists to prevent, so it is the last
    thing that may be taken on trust.
    """
    artifacts = doc.get("artifacts")
    if not artifacts:
        raise Stale(f"{where}: no artifacts block")
    for name, art in sorted(artifacts.items()):
        recipe = (art.get("recipe") or "").strip()
        if not recipe:
            raise Stale(f"{where}: artifact {name!r} has an empty recipe")
        if PLACEHOLDER_RE.search(recipe.replace(STAGE_TOKEN, "")):
            raise Stale(
                f"{where}: artifact {name!r} recipe is incomplete "
                f"(unexpanded placeholder): {recipe!r}"
            )
        for field in ("sha256", "bytes"):
            if art.get(field) in (None, "", "…"):
                raise Stale(f"{where}: artifact {name!r} has no {field}")


def check_fingerprint(baseline: dict, capture: dict) -> None:
    """A delta across a fingerprint change is uninterpretable, so refuse it.

    This is the machine-checked form of the rule the 2026-09-07 provenance
    audit named and left unhomed: a record that does not pin its harness lets
    reference drift pass as a measurement. The lane cannot compare numbers it
    cannot attribute, so a mismatch reports stale rather than guessing.
    """
    want = baseline.get("fingerprint")
    got = capture.get("fingerprint")
    if not want:
        raise Stale("baseline: no fingerprint block")
    if not got:
        raise Fault("capture: no fingerprint block")

    moved = [
        f"  {key}: baseline {want[key]!r} != capture {got.get(key)!r}"
        for key in sorted(want)
        if got.get(key) != want[key]
    ]
    missing = sorted(set(want) - set(got))
    if missing:
        moved.append(f"  absent from capture: {', '.join(missing)}")
    if moved:
        raise Stale("the environment moved:\n" + "\n".join(moved))


def check_alloc_readers(rounds: list[dict]) -> None:
    """`-1` means the reader was absent, not that the value was zero."""
    for index, row in enumerate(rounds, start=1):
        for field in (
            "alloc_count_per_cycle",
            "alloc_bytes_per_cycle",
            "peak_live_bytes",
        ):
            if row.get(field) is None:
                raise Fault(f"G3 round {index}: {field} missing from the capture")
            if row[field] == -1:
                raise Fault(
                    f"G3 round {index}: {field} is -1 — the oxpinyin_alloc_* reader "
                    "was absent, so the artifact was built without "
                    "--features alloc-count. This is not a measurement of zero."
                )


def check_agreement(metric: str, rounds: list[dict]) -> None:
    """Round 2 exists to falsify round 1. Disagreement is a fault, not a finding."""
    if len(rounds) < 2:
        raise Fault(f"{metric.upper()}: expected 2 rounds, got {len(rounds)}")
    mode, tolerance = AGREEMENT[metric]
    first, second = rounds[0], rounds[1]
    for field in sorted(first):
        if field not in second:
            raise Fault(f"{metric.upper()}: round 2 lacks {field}")
        a, b = first[field], second[field]
        if not isinstance(a, (int, float)) or not isinstance(b, (int, float)):
            continue
        if mode == "exact":
            if a != b:
                raise Fault(
                    f"{metric.upper()} {field}: rounds disagree ({a} vs {b}); "
                    "this metric is deterministic, so a difference means the "
                    "instrument, not the code"
                )
        else:
            if a == 0:
                if b != 0:
                    raise Fault(f"{metric.upper()} {field}: rounds disagree (0 vs {b})")
                continue
            drift = abs(b - a) / abs(a)
            if drift > tolerance:
                raise Fault(
                    f"{metric.upper()} {field}: rounds disagree by {drift:.4%}, "
                    f"over the {tolerance:.2%} floor ({a} vs {b})"
                )


def compared_value(metric: str, rounds: list[dict], field: str) -> float:
    """Round 1 for the exact metrics; the median of passes for G4."""
    if metric == "g4":
        return statistics.median(row[field] for row in rounds)
    return rounds[0][field]


def compare(baseline: dict, capture: dict, nightly: bool) -> list[str]:
    """Apply each gate's threshold. Returns the regressions found, in order."""
    base = baseline["metrics"]
    rounds = capture["rounds"]
    failures: list[str] = []
    warnings: list[str] = []

    def value(metric: str, field: str) -> float:
        return compared_value(metric, rounds[metric], field)

    # G1 — size. The section sum is the primary gate: stripped file size is
    # page-quantized (perf-so-size-2026-09.md measured seven distinct probe
    # cdylibs all reporting an identical 266,736 B), so a real 4 KiB .text
    # growth can land with a zero file-size delta.
    base_sum = base["g1"]["section_sum"]
    limit = base_sum + max(base_sum * 0.005, 4096)
    actual_sum = value("g1", "section_sum")
    if actual_sum > limit:
        failures.append(
            f"G1 section_sum {actual_sum} > limit {limit:.0f} "
            f"(baseline {base_sum} + max(0.5%, 4096 B))"
        )
    base_stripped = base["g1"]["stripped_size"]
    actual_stripped = value("g1", "stripped_size")
    if actual_stripped > base_stripped:
        failures.append(
            f"G1 stripped_size {actual_stripped} > baseline {base_stripped}"
        )

    # G2 — instructions, oxpinyin object only. Excluding glibc and libtkrzw
    # means a system-library change cannot move the gated number, and cannot
    # mask a regression in our own code either.
    base_ir = base["g2"]["ir_oxpinyin_object"]
    actual_ir = value("g2", "ir_oxpinyin_object")
    if base_ir:
        growth = (actual_ir - base_ir) / base_ir
        if growth > 0.02:
            failures.append(
                f"G2 ir_oxpinyin_object +{growth:.3%} over baseline "
                f"({actual_ir} vs {base_ir}), limit +2.0%"
            )
        elif growth > 0.005:
            warnings.append(
                f"G2 ir_oxpinyin_object +{growth:.3%} over baseline "
                f"({actual_ir} vs {base_ir}) — warn at +0.5%, fail at +2.0%"
            )

    # G3 — allocations. An exact ratchet: the count is a pure function of the
    # code path, so any increase is deliberate or it is a regression.
    base_count = base["g3"]["alloc_count_per_cycle"]
    actual_count = value("g3", "alloc_count_per_cycle")
    if actual_count > base_count:
        failures.append(
            f"G3 alloc_count_per_cycle {actual_count} > baseline {base_count} "
            "(exact ratchet: allocations per cycle may not increase)"
        )
    base_bytes = base["g3"]["alloc_bytes_per_cycle"]
    actual_bytes = value("g3", "alloc_bytes_per_cycle")
    if base_bytes and (actual_bytes - base_bytes) / base_bytes > 0.01:
        failures.append(
            f"G3 alloc_bytes_per_cycle {actual_bytes} > baseline {base_bytes} +1%"
        )

    # G4 — RSS. Reported on PRs, gated on the nightly: it moves with glibc, the
    # allocator and the host kernel, and the kernel is not pinned by a
    # container image.
    for field in ("rss_init_kib", "rss_cycle_kib"):
        base_rss = base["g4"][field]
        actual_rss = value("g4", field)
        if not base_rss:
            continue
        growth = (actual_rss - base_rss) / base_rss
        if growth > 0.03:
            message = (
                f"G4 {field} +{growth:.3%} over baseline "
                f"({actual_rss} vs {base_rss}), limit +3.0%"
            )
            if nightly:
                failures.append(message)
            else:
                warnings.append(message + " — reported only; G4 gates nightly")

    for line in warnings:
        print(f"warning: {line}")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--capture", required=True, type=Path)
    parser.add_argument(
        "--nightly",
        action="store_true",
        help="gate G4 as well; on a PR it is reported and not gated",
    )
    args = parser.parse_args()

    try:
        baseline = load(args.baseline)
        capture = load(args.capture)

        # Stale before fault before regression: a baseline that does not
        # describe this environment makes every downstream number
        # uninterpretable, so there is nothing to fault or compare.
        check_recipes(baseline, "baseline")
        check_recipes(capture, "capture")
        check_fingerprint(baseline, capture)

        rounds = capture.get("rounds")
        if not rounds:
            raise Fault("capture: no rounds block")
        for metric in ("g1", "g2", "g3", "g4"):
            if metric not in rounds:
                raise Fault(f"capture: no {metric.upper()} rounds")

        check_alloc_readers(rounds["g3"])
        for metric in ("g1", "g2", "g3", "g4"):
            check_agreement(metric, rounds[metric])

    except Stale as exc:
        print(f"BASELINE STALE: {exc}", file=sys.stderr)
        print(
            "  refresh the baseline from this run's uploaded artifact, in its "
            "own change, then rebase.",
            file=sys.stderr,
        )
        return EXIT_STALE
    except Fault as exc:
        print(f"INSTRUMENT FAULT: {exc}", file=sys.stderr)
        print(
            "  the measurement is not trustworthy; this is not a statement "
            "about the code under test.",
            file=sys.stderr,
        )
        return EXIT_FAULT

    failures = compare(baseline, capture, args.nightly)
    if failures:
        print("REGRESSION:", file=sys.stderr)
        for line in failures:
            print(f"  {line}", file=sys.stderr)
        return EXIT_REGRESSION

    print("perf-gate: every gated value within threshold")
    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main())
