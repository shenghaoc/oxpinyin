"""Path-based CI job selection and aggregate policy."""
from __future__ import annotations

from dataclasses import dataclass
import argparse
import fnmatch
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from typing import Any


JOB_FLAGS = {"rust": "rust", "store": "store", "shipping-sizes": "ship"}


@dataclass(frozen=True)
class Inputs:
    """The paths one ci.yml job reads.

    A path selects the job when it matches `paths` and does not match `skip`,
    or when it matches `keep`. An entry with `*` is a glob whose `*` also
    matches `/`; any other entry matches itself and every path under it.
    """
    paths: tuple[str, ...]
    skip: tuple[str, ...] = ()
    keep: tuple[str, ...] = ()

    def selects(self, path: str) -> bool:
        return matches(path, self.paths) and (not matches(path, self.skip) or matches(path, self.keep))


def matches(path: str, entries: tuple[str, ...]) -> bool:
    return any(fnmatch.fnmatchcase(path, entry) if "*" in entry
               else path == entry or path.startswith(entry + "/") for entry in entries)


# Every job reads its workflow, the actions, the toolchain and the tool configuration.
# pyproject.toml is the uv project that every `uv run` of repository Python enters.
SHARED = (".github/workflows/ci.yml", ".github/actions", "rust-toolchain.toml", ".cargo",
          "justfile", "uv.toml", ".python-version", "pyproject.toml")


def workspace_members(root: Path = Path(__file__).resolve().parents[2]) -> tuple[str, ...]:
    """The member directories in the `[workspace] members` list of the root Cargo.toml."""
    with (root / "Cargo.toml").open("rb") as manifest:
        return tuple(tomllib.load(manifest)["workspace"]["members"])


CRATE_DIRS = workspace_members()
CRATE_INPUTS = (*CRATE_DIRS, "src", "tests", "codegen", "build.rs", "benches", "examples")
# clippy.toml configures every clippy run, so it selects the Rust lanes.
RUST = (*SHARED, *CRATE_INPUTS, "Cargo.toml", "Cargo.lock", "clippy.toml", "fixtures", "tools")
# The tools/ paths that no Rust lane imports, runs or reads. `python -m tools.dev`
# imports every other tools/dev module, so a change there still selects the Rust lanes.
PYTHON_ONLY = ("tools/bisection/*.py",
               "tools/oracle", "tools/scripts2")
# The exceptions: `python -m tools.dev` imports these modules, and the Rust lanes read the pin records.
RUST_READS = (
              "tools/oracle/build.py", "tools/oracle/image.py", "tools/oracle/versions.py",
              "tools/oracle/oracle-pin.txt", "tools/oracle/libpinyin-data.json")
# The shipping-size gate selects the inputs of the shipped libraries without a Cargo toolchain:
# the root library and the two C API crates. The other workspace members, the root tests and
# each C API crate's integration tests are in no shipped library.
SHIP_CRATES = ("capi", "zhuyin-capi")
SHIP = ("src", "codegen", "build.rs", *SHIP_CRATES, "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo",
        "tools/dev/packaging.py",
        "tools/dev/packaging_templates.py", "tools/oracle/oracle-pin.txt",
        "justfile", "pyproject.toml", "tools/dev/cell.py", "tools/dev/ship_sizes.py",
        ".github/workflows/ci.yml", ".github/actions")
CI_INPUTS = {
    # cargo fmt and fuzz Clippy read the fuzz workspace. The
    # complexity check runs Lizard from a dependency group, so the lockfile is
    # an input. `just deny` runs cargo deny, which reads the manifests, the
    # lockfiles and deny.toml.
    "rust": Inputs((*RUST, "fuzz", "uv.lock", ".config/nextest.toml", "deny.toml"), PYTHON_ONLY,
                   RUST_READS),
    # This job runs without Cargo, and ship_sizes keys the base result on the same crates.
    "ship": Inputs(SHIP, tuple(f"{directory}/tests" for directory in SHIP_CRATES)),
}


def event_base(event_name: str, event: dict[str, Any]) -> str:
    """The pull request's base commit, or the previous head of a push."""
    if event_name == "pull_request":
        return (event.get("pull_request") or {}).get("base", {}).get("sha", "")
    return event.get("before", "")


def gate(base: str, inputs: dict[str, Inputs] = CI_INPUTS, root: Path | None = None) -> dict[str, bool]:
    """Select each job whose inputs differ between base and HEAD.

    The diff is two-dot: a branch behind main also sees main's own changes,
    which can only select more jobs. An unavailable base selects every job.
    """
    command = ["git", "diff", "--name-only", "--no-renames", "-z", base, "HEAD", "--"]
    done = None
    if SHA.fullmatch(base):
        done = subprocess.run(command, cwd=root, capture_output=True, text=True)
    if done is None or done.returncode:
        return dict.fromkeys(inputs, True)
    changed = done.stdout.split("\0")[:-1]
    return {name: any(lane.selects(path) for path in changed) for name, lane in inputs.items()}


# The store job's gate: docs/runbooks/verification.md, rule 2, as
# tools/bisection/cells-for-diff.py implements it for local runs, plus the store
# job's own infrastructure as --also paths: the workflow, the utilities'
# per-cell tests and their data under utils, the runtime check under
# tools/tkrzw, the C ABI gate and both libraries it loads, the sanitizer service
# (tools/dev/test_runner.py), the toolchain pins (the sanitizer nightly is in
# fuzz/rust-toolchain.toml), clippy.toml (the store job runs clippy for the
# peer backends), nextest's configuration and this gate's own code.
# Every Python step enters the uv project in pyproject.toml. --run-without-base
# makes an empty base, one that is not a commit here or one with no merge base
# select the store job, the over-trigger direction.
STORE_GATE_SCRIPT = Path(__file__).resolve().parents[1] / "bisection" / "cells-for-diff.py"
STORE_GATE_ARGUMENTS = ("--run-flag", "--run-without-base", *(argument for path in (
    *SHARED, "utils/", "tool-support/", "fixtures/w3/kct/", "fixtures/w3/db/",
    "src/", "codegen/", "capi/", "zhuyin-capi/", "Cargo.toml", "tools/abi/",
    "tools/dev/abi.py", "tools/dev/abi_headers.py", "tools/dev/cell.py", "tools/dev/drivers.py",
    "tools/dev/entry.py", "tools/dev/host.py", "tools/dev/native.py", "tools/dev/pool.py",
    "tools/dev/slot.py", "tools/dev/runners.py", "tools/dev/packaging.py", "tools/oracle/oracle-pin.txt",
    "tools/dev/ci_tree.py", "tools/bisection/cells-for-diff.py",
    "tools/tkrzw", "tools/dev/check.py",
    "tools/dev/test_runner.py", "fuzz/rust-toolchain.toml",
    "clippy.toml", ".config/nextest.toml") for argument in ("--also", path)))


def store_gate(base: str, root: Path | None = None) -> bool:
    """Whether the store gate selects the store job for base...HEAD.

    The script's diagnostics reach stderr. A failure of the script raises
    CalledProcessError, so the gate step fails rather than skip the jobs.
    """
    done = subprocess.run([sys.executable, "-X", "utf8", str(STORE_GATE_SCRIPT), *STORE_GATE_ARGUMENTS, base],
                          cwd=root, stdout=subprocess.PIPE, text=True, check=True)
    if done.stdout not in {"run=true\n", "run=false\n"}:
        raise ValueError(f"unexpected store gate output {done.stdout!r}")
    return done.stdout == "run=true\n"


SHA = re.compile(r"[0-9a-f]{40}")


def aggregate(changes: str, flags: dict[str, str], results: dict[str, str]) -> bool:
    """Require selected jobs to succeed and unselected jobs to be skipped."""
    return (
        changes == "success"
        and set(flags) == set(results)
        and all(flag in {"true", "false"} for flag in flags.values())
        and all(result == ("success" if flags[name] == "true" else "skipped")
                for name, result in results.items())
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("aggregate", help="check each lane's result against its flag")
    commands.add_parser("gate", help="write each ci.yml job's path-gate flag to GITHUB_OUTPUT")
    args = parser.parse_args()
    if args.command == "gate":
        event_name = os.environ["GITHUB_EVENT_NAME"]
        base = event_base(event_name, json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text()))
        flags = {**gate(base), "store": store_gate(base)}
        lines = "".join(f"{name}={'true' if value else 'false'}\n" for name, value in flags.items())
        print(f"base {base or '(none)'}\n{lines}", end="")
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(lines)
        return 0
    needs = json.loads(os.environ["NEEDS"])
    results = {name: needs[name]["result"] for name in JOB_FLAGS}
    outputs = needs["changes"]["outputs"]
    flags = {name: outputs.get(flag, "") for name, flag in JOB_FLAGS.items()}
    passed = aggregate(needs["changes"]["result"], flags, results)
    print(json.dumps({"changes": needs["changes"], "lanes": results, "passed": passed}))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
