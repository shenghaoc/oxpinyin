#!/usr/bin/env python3
"""callgrind-ir.py — total Ir attributed to one object in a callgrind file.

Used by tools/perf-gate/snapshot.sh for the nightly series' instruction count.
It is a separate file rather than an inline heredoc so it can be tested against
fixtures: tools/perf-gate/callgrind-ir.test.sh.

The format has three features that a naive "sum the numbers under a matching
`ob=`" reader gets wrong, and all three were wrong in the first version:

1. **Name compression.** Callgrind writes `ob=(1) /path/lib.so` once and then
   refers to the same object as bare `ob=(1)`. A reader that tests the line
   text for a substring sees the definition and then silently drops every
   later record for that object — under-counting, with no error. The ids are
   resolved here through a table built from the definitions.

2. **Call cost lines are inclusive.** After a `calls=` line, the next cost
   line is the *inclusive* cost of that call, not self cost in this function.
   Adding it double-counts everything reachable from the call. Those lines are
   skipped.

3. **The columns are declared, not fixed.** `events:` names them in order, and
   `Ir` is only the first when callgrind was run without extra simulations
   (`--cache-sim=yes` prepends nothing but adds columns after). `positions:`
   likewise says how many position fields precede the costs — two when
   `--dump-instr=yes` is in effect. Both are read rather than assumed.

Prints the integer total, or `null` when the file declares no `Ir` event or
nothing was attributed to the object.

Usage: callgrind-ir.py <callgrind-out-file> <object-substring>
"""

from __future__ import annotations

import sys


def object_ir(path: str, want: str) -> int | None:
    """Sum self-Ir for records whose object name contains `want`."""
    events: list[str] = []
    n_positions = 1  # `positions: line` is the default
    ir_index: int | None = None

    objects: dict[str, str] = {}  # compression id -> object name
    current_object = ""
    skip_next_cost = False  # the line after `calls=` is an inclusive cost
    total = 0
    matched = False

    with open(path, errors="replace") as handle:
        for raw in handle:
            line = raw.rstrip("\n")
            if not line or line.startswith("#"):
                continue

            if line.startswith("events:"):
                events = line.split(":", 1)[1].split()
                ir_index = events.index("Ir") if "Ir" in events else None
                continue
            if line.startswith("positions:"):
                n_positions = len(line.split(":", 1)[1].split()) or 1
                continue

            if line.startswith("ob=") or line.startswith("cob="):
                body = line.split("=", 1)[1].strip()
                # Forms: "(1) /path/lib.so" (definition), "(1)" (reference),
                # or a bare "/path/lib.so" with compression disabled.
                if body.startswith("("):
                    close = body.find(")")
                    ident, name = body[1:close], body[close + 1:].strip()
                    if name:
                        objects[ident] = name
                    else:
                        name = objects.get(ident, "")
                else:
                    name = body
                # `cob=` names the callee's object and does not change which
                # object the following self-cost lines belong to.
                if line.startswith("ob="):
                    current_object = name
                continue

            if line.startswith("calls="):
                skip_next_cost = True
                continue

            # Anything else that starts with a digit, a sub-position sign, or
            # `*` is a cost line.
            if line[0].isdigit() or line[0] in "+-*":
                if skip_next_cost:
                    skip_next_cost = False
                    continue
                if ir_index is None or want not in current_object:
                    continue
                fields = line.split()
                cost_index = n_positions + ir_index
                if cost_index < len(fields):
                    value = fields[cost_index]
                    # Sub-position compression can leave a non-numeric token in
                    # a position column; costs themselves are plain integers.
                    if value.isdigit():
                        total += int(value)
                        matched = True
                continue

            # Any other header (fl=, fn=, cfn=, summary:, …) ends a call
            # record without consuming its cost line.
            skip_next_cost = False

    return total if matched else None


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__.strip().split("\n")[-1], file=sys.stderr)
        return 2
    try:
        total = object_ir(sys.argv[1], sys.argv[2])
    except OSError as exc:
        print(f"callgrind-ir: {exc}", file=sys.stderr)
        return 2
    print("null" if total is None else total)
    return 0


if __name__ == "__main__":
    sys.exit(main())
