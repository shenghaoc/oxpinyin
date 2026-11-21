"""Conservative tree-based CI reuse and shared aggregate policy.

Evidence is a successful PR workflow run, never an arbitrary commit status.
It must hold a successful job for every lane the current run's path gate
selects; a lane the current run does not select needs no evidence. The
marker's tree is resolved by checkout, so a PR merge ref and its head
cannot accidentally be treated as the same tree. Reused runs reference their
original source, whose verification age is never renewed by a duplicate.
Only the latest run for a candidate head is considered; incomplete, failed,
missing, ambiguous or unavailable evidence falls back to normal verification.
"""
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
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
from urllib.error import URLError
from urllib.parse import urlencode
from urllib.request import Request, urlopen

from . import cell


@dataclass(frozen=True)
class Workflow:
    path: str
    aggregate: str
    # Each lane is a ci.yml job whose id is also its name, so a needs key and an
    # API job name are the same string.
    lanes: tuple[str, ...]
    # The `changes` output flag of each lane in `lanes`.
    flags: tuple[str, ...]
    # The lanes that still compile on a reused push to main, to refresh its Rust cache.
    cached: frozenset[str]
    # (job, `changes` output flag): lanes with their own path gate. They are not
    # in `lanes`, so a skipped one never blocks tree reuse, and `reuse` never
    # skips one that applies.
    gated: tuple[tuple[str, str], ...] = ()


WORKFLOW = Workflow(
    ".github/workflows/ci.yml", "ci-aggregate",
    ("rust", "harness", "store"),
    ("rust", "harness", "store"),
    frozenset({"rust", "store"}),
    (("shipping-sizes", "ship"),))


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
PYTHON_ONLY = ("tools/*.test.py", "tools/bisection/*.py",
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
# The Rust files `python -m tools.dev.rust_tables` writes. A harness test regenerates them and
# compares, so a change to one selects the harness. The same test holds this list to the generator's.
GENERATED_TABLES = (
    "src/chewing_key_tables.rs",
    "src/content_table_source.rs",
    "src/tone_symbols.rs",
    "src/table_data/double_pinyin.rs",
    "src/table_data/full_pinyin_counts.rs",
    "src/table_data/full_pinyin_indexes.rs",
    "src/table_data/full_pinyin_syllables.rs",
    "src/table_data/syllable_inventory.rs",
    "src/table_data/tone_symbols.rs",
    "src/table_data/zhuyin_indexes.rs",
    "src/table_data/zhuyin_keyboards.rs",
    "src/table_data/zhuyin_tones.rs",
    "src/engine/session/split_tables.rs",
)
CI_INPUTS = {
    # cargo fmt and the compile-only fuzz check read the fuzz workspace. The
    # complexity check runs Lizard from a dependency group, so the lockfile is
    # an input. `just deny` runs cargo deny, which reads the manifests, the
    # lockfiles and deny.toml.
    "rust": Inputs((*RUST, "fuzz", "uv.lock", ".config/nextest.toml", "deny.toml"), PYTHON_ONLY,
                   (*RUST_READS, "tools/dev/lint-crate-cfg-placement.test.py")),
    # tools/ and what its tests read outside it: the workflow, the manifests
    # and the lockfile, the files directly in each crate's directory (version
    # scripts, headers, allocator registers, build scripts), the generated Rust
    # tables, the fixtures, the table-conf template, the sanitizer nightly, the
    # fuzz lockfile (its afl version is the cargo-afl version), the fuzz seeds
    # and dictionaries (the fuzz tests read the curated inputs and the
    # table-conf dictionary), and the lockfile of the dependency group that
    # holds Ruff. No harness test
    # reads any other Rust source, so the files under a crate's src, tests,
    # examples and benches do not select the job.
    # The Python version request rule reads every tracked file, so it is a step
    # of the `changes` job, not an input here.
    "harness": Inputs((*SHARED, "uv.lock",
                       "tools", "Cargo.toml", "Cargo.lock",
                       *CRATE_INPUTS, "fixtures", "data/table.conf.in", "fuzz/rust-toolchain.toml",
                       "fuzz/Cargo.lock", "fuzz/seeds", "fuzz/dictionaries"),
                      tuple(f"{prefix}{section}/*.rs"
                            for prefix in ("", *(f"{directory}/" for directory in CRATE_DIRS))
                            for section in ("src", "tests", "examples", "benches")),
                      GENERATED_TABLES),
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
    *SHARED, "utils/", "fixtures/w3/kct/", "fixtures/w3/db/",
    "src/", "codegen/", "capi/", "zhuyin-capi/", "Cargo.toml", "tools/abi/",
    "tools/dev/abi.py", "tools/dev/abi_headers.py", "tools/dev/cell.py", "tools/dev/drivers.py",
    "tools/dev/entry.py", "tools/dev/host.py", "tools/dev/native.py", "tools/dev/pool.py",
    "tools/dev/slot.py", "tools/dev/runners.py", "tools/dev/packaging.py", "tools/oracle/oracle-pin.txt",
    "tools/dev/ci_tree.py", "tools/dev/ci_tree.test.py", "tools/bisection/cells-for-diff.py",
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
# Strict up-to-date checks constrain Git ancestry, not runner-image drift.
# Bound every original verification job, including its execution time.
EVIDENCE_MAX_AGE = timedelta(hours=24)


def git_tree(revision: str) -> str:
    return subprocess.check_output(
        ["git", "rev-parse", "--verify", revision + "^{tree}"], text=True).strip()


class GitHub:
    """Read only the repository API; never follow supplied evidence URLs."""

    def __init__(self, repository: str, token: str):
        self.base = "https://api.github.com/repos/" + repository
        self.token = token

    def get(self, path: str) -> Any:
        request = Request(self.base + path, headers={
            "Authorization": "Bearer " + self.token,
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
        })
        with urlopen(request, timeout=20) as response:
            return json.load(response)

    def pages(self, path: str, key: str | None = None) -> list[dict[str, Any]]:
        result: list[dict[str, Any]] = []
        # Bounded lookups fail closed rather than trusting truncated evidence.
        for page in range(1, 11):
            separator = "&" if "?" in path else "?"
            data = self.get(path + separator + urlencode({"per_page": 100, "page": page}))
            items = data[key] if key else data
            if not isinstance(items, list):
                raise ValueError("invalid API collection")
            result.extend(items)
            if len(items) < 100:
                return result
        raise ValueError("evidence pagination limit exceeded")


def run_provenance(run: dict[str, Any], workflow: Workflow, repository_id: int,
                   pr_number: int, current_run: int) -> bool:
    """Match repository and PR identity before considering a same-named branch."""
    return (
        run.get("id") != current_run
        and run.get("event") == "pull_request"
        and run.get("path") == workflow.path
        and (run.get("repository") or {}).get("id") == repository_id
        and (run.get("head_repository") or {}).get("id") == repository_id
        and any((pr or {}).get("number") == pr_number for pr in run.get("pull_requests") or [])
    )


def trusted_run(run: dict[str, Any], workflow: Workflow, repository_id: int,
                pr_number: int, head: str, current_run: int) -> bool:
    """Require applicable provenance and a completed successful latest attempt."""
    return (
        run_provenance(run, workflow, repository_id, pr_number, current_run)
        and run.get("head_sha") == head
        and run.get("status") == "completed"
        and run.get("conclusion") == "success"
        and isinstance(run.get("run_attempt"), int)
        and run["run_attempt"] >= 1
    )


def fresh_job(job: dict[str, Any], now: datetime) -> bool:
    """Require valid original execution timestamps within the reuse window."""
    try:
        started = datetime.fromisoformat(job["started_at"])
        completed = datetime.fromisoformat(job["completed_at"])
        if started.tzinfo is None or completed.tzinfo is None:
            return False
        return now - EVIDENCE_MAX_AGE <= started <= completed <= now
    except (KeyError, TypeError, ValueError):
        return False


def selected_lanes(workflow: Workflow, flags: dict[str, str]) -> tuple[str, ...]:
    """The lanes of workflow whose path-gate flag is true in the current run.

    A flag that is missing or is not `true` or `false` raises ValueError, so
    the caller finds no evidence and normal verification runs.
    """
    if any(flags.get(flag) not in {"true", "false"} for flag in workflow.flags):
        raise ValueError("invalid path-gate flags")
    return tuple(lane for lane, flag in zip(workflow.lanes, workflow.flags) if flags[flag] == "true")


def complete_jobs(jobs: list[dict[str, Any]], workflow: Workflow, tree: str,
                  now: datetime | None = None, lanes: tuple[str, ...] | None = None) -> bool:
    """Every lane in lanes, the aggregate and the checkout-derived marker must have succeeded.

    lanes holds the lanes the current run selects; None means every lane. A
    selected lane that the source run skipped is insufficient, whatever its
    aggregate says. A lane outside lanes is not read. Duplicate names are
    ambiguous, including attempts mixed by an incorrect API query.
    """
    now = now or datetime.now(timezone.utc)
    if lanes is None:
        lanes = workflow.lanes
    if not set(lanes) <= set(workflow.lanes):
        return False
    names = (*lanes, workflow.aggregate, "tested tree " + tree)
    return all(
        len(matches := [job for job in jobs if job.get("name") == name]) == 1
        and matches[0].get("status") == "completed"
        and matches[0].get("conclusion") == "success"
        and fresh_job(matches[0], now)
        for name in names)


def full_source(api: GitHub, run: dict[str, Any], jobs: list[dict[str, Any]],
                workflow: Workflow, tree: str, repository_id: int,
                pr_number: int, current_run: int, now: datetime | None = None,
                lanes: tuple[str, ...] | None = None) -> int | None:
    """Resolve a reused PR run directly to its original verification.

    A successful duplicate must not hide its source from the subsequent
    main push. Skipped lanes alone never count as evidence: require the
    workflow-issued source marker, then revalidate the original run against
    the lanes the current run selects.
    """
    now = now or datetime.now(timezone.utc)
    if complete_jobs(jobs, workflow, tree, now, lanes):
        return int(run["id"])
    for name in (*workflow.lanes, workflow.aggregate):
        matches = [job for job in jobs if job.get("name") == name]
        expected = "success" if name == workflow.aggregate else "skipped"
        if len(matches) != 1 or matches[0].get("status") != "completed" or matches[0].get("conclusion") != expected:
            return None
    pattern = re.compile(r"reused tree " + tree + r" from run ([0-9]+)")
    markers = [(job, pattern.fullmatch(job.get("name", ""))) for job in jobs]
    markers = [(job, match) for job, match in markers if match]
    if len(markers) != 1:
        return None
    marker, match = markers[0]
    source_id = int(match[1])
    if marker.get("status") != "completed" or marker.get("conclusion") != "success" or source_id >= run["id"]:
        return None
    source = api.get(f"/actions/runs/{source_id}")
    head = source.get("head_sha", "")
    if not SHA.fullmatch(head) or not trusted_run(source, workflow, repository_id, pr_number, head, current_run):
        return None
    if api.get("/git/commits/" + head).get("tree", {}).get("sha") != tree:
        return None
    source_jobs = api.pages(f"/actions/runs/{source_id}/attempts/{source['run_attempt']}/jobs", "jobs")
    return source_id if complete_jobs(source_jobs, workflow, tree, now, lanes) else None


def find_evidence(api: GitHub, workflow: Workflow, event: dict[str, Any],
                  event_name: str, tree: str, repository_id: int,
                  current_run: int, now: datetime | None = None,
                  lanes: tuple[str, ...] | None = None) -> int | None:
    now = now or datetime.now(timezone.utc)
    if event_name == "push" and event.get("ref") == "refs/heads/main":
        sha = event["after"]
        if not SHA.fullmatch(sha):
            raise ValueError("invalid pushed commit")
        prs = api.pages("/commits/" + sha + "/pulls")
        prs = [pr for pr in prs if (pr or {}).get("merged_at")]
    elif event_name == "pull_request":
        prs = [event["pull_request"]]
    else:
        return None
    for pr in prs:
        pr = pr or {}
        base = pr.get("base") or {}
        head_info = pr.get("head") or {}
        if (base.get("ref") != "main"
                or (base.get("repo") or {}).get("id") != repository_id
                or (head_info.get("repo") or {}).get("id") != repository_id):
            continue
        head = head_info.get("sha", "")
        if not SHA.fullmatch(head):
            continue
        # The commit hash locates evidence; only tree equality authorizes reuse.
        commit = api.get("/git/commits/" + head)
        if commit.get("tree", {}).get("sha") != tree:
            continue
        runs = api.pages("/actions/workflows/" + workflow.path.rsplit("/", 1)[1]
                         + "/runs?" + urlencode({"event": "pull_request", "branch": head_info["ref"]}),
                         "workflow_runs")
        # Only the newest run for this exact tree is eligible. An older green
        # must never hide a newer failed/pending attempt for the same tree.
        run = None
        matching = [run for run in runs if run_provenance(
            run, workflow, repository_id, pr["number"], current_run)]
        for candidate in sorted(matching, key=lambda item: item["id"], reverse=True)[:20]:
            candidate_head = candidate.get("head_sha", "")
            if not SHA.fullmatch(candidate_head):
                continue
            candidate_commit = api.get("/git/commits/" + candidate_head)
            if candidate_commit.get("tree", {}).get("sha") == tree:
                run = candidate
                break
        if run is None or not trusted_run(run, workflow, repository_id, pr["number"],
                                          run["head_sha"], current_run):
            continue
        jobs = api.pages(f"/actions/runs/{run['id']}/attempts/{run['run_attempt']}/jobs", "jobs")
        source = full_source(api, run, jobs, workflow, tree, repository_id, pr["number"], current_run, now, lanes)
        if source is not None:
            return source
    return None


def aggregate(changes: str, flags: dict[str, str], reuse: str,
              results: dict[str, str], cached: frozenset[str], event_name: str = "push",
              own_gate: dict[str, tuple[str, str]] | None = None) -> bool:
    """Failures and unjustified skips fail; cancellation is handled by job if.

    flags maps each lane in results to its path-gate flag. own_gate maps a lane
    to (result, flag). Such a lane must succeed when its flag is true, whatever
    `reuse` says, and be skipped when it is false.
    """
    if (changes != "success" or set(flags) != set(results) or reuse not in {"true", "false"}
            or any(flag not in {"true", "false"} for flag in flags.values())):
        return False
    for result, flag in (own_gate or {}).values():
        if flag not in {"true", "false"} or result != ("success" if flag == "true" else "skipped"):
            return False
    for name, result in results.items():
        expected = "success" if flags[name] == "true" and (
            reuse != "true" or (event_name == "push" and name in cached)) else "skipped"
        if result != expected:
            return False
    return True


@dataclass(frozen=True)
class CompileCommand:
    argv: tuple[str, ...]
    environment: tuple[tuple[str, str], ...] = ()


CACHE_LANES = tuple(sorted(WORKFLOW.cached))
# The non-default backends the store job builds with the stable toolchain.
# The backend of every Linux job: the one Linux cell, Kyoto Cabinet from Ubuntu's packages
# (LOCAL_CELL in tools/dev/cell.py; docs/runbooks/verification.md, section 10). The `rust`
# lane selects it with the backend flag; the store job's workspace build is the remaining peer.
LINUX_CI_BACKEND = cell.CELL_TO_BACKEND[cell.LOCAL_CELL]
STORE_PEERS = ("bdb",)


def refresh_commands(lane: str) -> tuple[CompileCommand, ...]:
    """The cache-saving lanes' compile modes; never the invalid-feature probes."""
    if lane not in CACHE_LANES:
        raise ValueError("invalid cache refresh selection")
    if lane == "rust":
        flags = cell.feature_flags(LINUX_CI_BACKEND)
        return (CompileCommand(("cargo", "clippy", "--locked", "--workspace", "--all-targets", *flags, "--", "-D", "warnings")),
                CompileCommand(("cargo", "build", "--locked", "--workspace", "--tests", *flags)))
    # store: the peer build into the job's one target directory.
    return tuple(CompileCommand(("cargo", "build", "--locked", "--workspace", "--tests", *cell.feature_flags(backend)))
                 for backend in STORE_PEERS)


def refresh_cache(lane: str) -> None:
    """Run shared cache refresh commands, inheriting the lane's Rust settings."""
    for command in refresh_commands(lane):
        environment = dict(os.environ)
        environment.update(command.environment)
        subprocess.run(command.argv, env=environment, check=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("evidence", help="write the tree-reuse outputs to GITHUB_OUTPUT")
    commands.add_parser("aggregate", help="check each lane's result against its flag")
    commands.add_parser("gate", help="write each ci.yml job's path-gate flag to GITHUB_OUTPUT")
    commands.add_parser("refresh").add_argument("lane", choices=CACHE_LANES)
    args = parser.parse_args()
    if args.command == "refresh":
        refresh_cache(args.lane)
        return 0
    if args.command == "gate":
        event_name = os.environ["GITHUB_EVENT_NAME"]
        base = event_base(event_name, json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text()))
        flags = {**gate(base), "store": store_gate(base)}
        lines = "".join(f"{name}={'true' if value else 'false'}\n" for name, value in flags.items())
        print(f"base {base or '(none)'}\n{lines}", end="")
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(lines)
        return 0
    workflow = WORKFLOW
    if args.command == "aggregate":
        needs = json.loads(os.environ["NEEDS"])
        results = {name: needs[name]["result"] for name in workflow.lanes}
        outputs = needs["changes"]["outputs"]
        flags = {name: outputs.get(flag, "") for name, flag in zip(workflow.lanes, workflow.flags)}
        own_gate = {name: (needs[name]["result"], outputs.get(flag, "")) for name, flag in workflow.gated}
        passed = aggregate(needs["changes"]["result"], flags,
                           outputs.get("reuse", ""), results, workflow.cached, os.environ["EVENT_NAME"], own_gate)
        print(json.dumps({"changes": needs["changes"], "lanes": results, "own_gate": own_gate,
                          "passed": passed}))
        return 0 if passed else 1
    tree = git_tree("HEAD")
    evidence = None
    try:
        # GATE_FLAGS holds the current run's path-gate outputs. Without it every lane is required.
        gate_flags = os.environ.get("GATE_FLAGS")
        lanes = None if gate_flags is None else selected_lanes(workflow, json.loads(gate_flags))
        api = GitHub(os.environ["GITHUB_REPOSITORY"], os.environ["GH_TOKEN"])
        evidence = find_evidence(
            api, workflow, json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text()),
            os.environ["GITHUB_EVENT_NAME"], tree, int(os.environ["GITHUB_REPOSITORY_ID"]),
            int(os.environ["GITHUB_RUN_ID"]), lanes=lanes)
    except (URLError, OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        print(f"No applicable complete evidence: {error}; running normal CI")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"source_run={evidence or ''}\ntree={tree}\nreuse={'true' if evidence else 'false'}\n")
    reason = (f"Reusing PR run {evidence} for exact tree `{tree}`: it passed every lane this run selects. "
              "Original verification is at most 24 hours old. "
              "Verification steps skip; main cache-saving lanes still compile."
              if evidence else f"No trusted PR run covers tree `{tree}`; normal path gates apply.")
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
        summary.write(reason + "\n")
    print(reason)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
