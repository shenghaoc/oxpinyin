"""Conservative tree-based CI reuse and shared aggregate policy.

Evidence is a full successful PR workflow, never an arbitrary commit status.
The marker's tree is resolved by checkout, so a PR merge ref and its head
cannot accidentally be treated as the same tree. Reused runs reference their
original full source, whose verification age is never renewed by a duplicate.
Only the latest run for a candidate head is considered; incomplete, failed,
missing, ambiguous or unavailable evidence falls back to normal verification.
"""
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
from typing import Any
from urllib.error import URLError
from urllib.parse import urlencode
from urllib.request import Request, urlopen


@dataclass(frozen=True)
class Workflow:
    path: str
    aggregate: str
    lanes: tuple[str, ...]
    cached: frozenset[str]
    needs: tuple[str, ...]
    skipped_lanes: tuple[str, ...]
    # (display name, needs job id, `changes` output flag): lanes with their own
    # path gate. They are not in `lanes`, so a skipped one never blocks tree
    # reuse, and `reuse` never skips one that applies.
    gated: tuple[tuple[str, str, str], ...] = ()


WORKFLOWS: dict[str, Workflow] = {
    "ci": Workflow(
        ".github/workflows/ci.yml", "ci-aggregate",
        ("lint", "test", "test-macos", "deny"),
        frozenset({"lint", "test", "test-macos"}),
        ("lint", "test", "test-macos", "deny"),
        ("lint", "test", "test-macos", "deny"),
        (("shipping-sizes", "shipping-sizes", "ship"),)),
    "store": Workflow(
        ".github/workflows/store-backends.yml",
        "store backends (kyotocabinet / tkrzw / bdb)",
        ("store backend (bdb)", "store backend (kyotocabinet)", "store backend (tkrzw)",
         "backend-matrix (exactly-one-backend gate)", "tkrzw FFI under address",
         "bdb backend sanitizers", "kc backend sanitizers"),
        frozenset({"store backend (bdb)", "store backend (kyotocabinet)", "store backend (tkrzw)",
                   "backend-matrix (exactly-one-backend gate)"}),
        ("store-features", "store-features", "store-features", "backend-matrix", "tkrzw-sanitizers",
         "bdb-sanitizers", "kc-sanitizers"),
        ("store backend (${{ matrix.backend }})", "backend-matrix (exactly-one-backend gate)",
         "tkrzw FFI under address", "bdb backend sanitizers", "kc backend sanitizers")),
}
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


def complete_jobs(jobs: list[dict[str, Any]], workflow: Workflow, tree: str,
                  now: datetime | None = None) -> bool:
    """All concrete lanes and the checkout-derived marker must have succeeded.

    A success aggregate with path-skipped lanes is insufficient. Duplicate
    names are ambiguous, including attempts mixed by an incorrect API query.
    """
    now = now or datetime.now(timezone.utc)
    names = (*workflow.lanes, workflow.aggregate, "tested tree " + tree)
    return all(
        len(matches := [job for job in jobs if job.get("name") == name]) == 1
        and matches[0].get("status") == "completed"
        and matches[0].get("conclusion") == "success"
        and fresh_job(matches[0], now)
        for name in names)


def full_source(api: GitHub, run: dict[str, Any], jobs: list[dict[str, Any]],
                workflow: Workflow, tree: str, repository_id: int,
                pr_number: int, current_run: int, now: datetime | None = None) -> int | None:
    """Resolve a reused PR run directly to its original full verification.

    A successful duplicate must not hide its full source from the subsequent
    main push. Skipped lanes alone never count as evidence: require the
    workflow-issued source marker, then revalidate the original full run.
    """
    now = now or datetime.now(timezone.utc)
    if complete_jobs(jobs, workflow, tree, now):
        return int(run["id"])
    for name in (*workflow.skipped_lanes, workflow.aggregate):
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
    return source_id if complete_jobs(source_jobs, workflow, tree, now) else None


def find_evidence(api: GitHub, workflow: Workflow, event: dict[str, Any],
                  event_name: str, tree: str, repository_id: int,
                  current_run: int, now: datetime | None = None) -> int | None:
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
        source = full_source(api, run, jobs, workflow, tree, repository_id, pr["number"], current_run, now)
        if source is not None:
            return source
    return None


def aggregate(changes: str, gated: str, reuse: str,
              results: dict[str, str], cached: frozenset[str], event_name: str = "push",
              own_gate: dict[str, tuple[str, str]] | None = None) -> bool:
    """Failures and unjustified skips fail; cancellation is handled by job if.

    own_gate maps a lane to (result, flag). Such a lane must succeed when its
    flag is true, whatever `reuse` says, and be skipped when it is false.
    """
    if changes != "success" or gated not in {"true", "false"} or reuse not in {"true", "false"}:
        return False
    for result, flag in (own_gate or {}).values():
        if flag not in {"true", "false"} or result != ("success" if flag == "true" else "skipped"):
            return False
    for name, result in results.items():
        expected = "success" if gated == "true" and (reuse != "true" or (event_name == "push" and name in cached)) else "skipped"
        if result != expected:
            return False
    return True


@dataclass(frozen=True)
class CompileCommand:
    argv: tuple[str, ...]
    environment: tuple[tuple[str, str], ...] = ()


CACHE_LANES = ("lint", "test", "test-macos", "store-features", "backend-matrix")
BACKENDS = ("bdb", "kyotocabinet", "tkrzw")
BACKEND_EXCLUDES = ("--exclude", "oxpinyin-kmm")
PORTABLE_EXCLUDES = ("--exclude", "oxpinyin-capi", "--exclude", "oxpinyin-zhuyin-capi",
                     *BACKEND_EXCLUDES)


def refresh_commands(lane: str, backend: str | None = None) -> tuple[CompileCommand, ...]:
    """The cache-saving lanes' compile modes; never the invalid-feature probes."""
    if lane not in CACHE_LANES or (backend is not None and lane != "store-features"):
        raise ValueError("invalid cache refresh selection")
    if lane == "lint":
        return (CompileCommand(("cargo", "clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings")),)
    if lane == "test":
        return (CompileCommand(("cargo", "build", "--locked", "--workspace", "--tests")),)
    if lane == "test-macos":
        return (CompileCommand(("cargo", "build", "--locked", "--workspace", "--tests",
                                "--no-default-features", "--features", "tkrzw", *PORTABLE_EXCLUDES)),)
    if lane == "store-features":
        if backend not in BACKENDS:
            raise ValueError("cache refresh requires a store backend")
        return (CompileCommand(("cargo", "build", "--locked", "--workspace", "--tests",
                                "--no-default-features", "--features", backend, *BACKEND_EXCLUDES)),)
    # Match the backend-matrix service's four valid package checks, including both
    # default and explicit tkrzw. Its invalid-combination checks remain in the
    # normal verification step. Rustdoc uses the lane's existing flags.
    check = ("cargo", "check", "--locked", "-p", "oxpinyin-store")
    checks = (CompileCommand(check), *(CompileCommand((*check, "--no-default-features", "--features", peer))
                                      for peer in ("kyotocabinet", "tkrzw", "bdb")))
    docs = tuple(CompileCommand(("cargo", "doc", "--locked", "--no-deps", "--workspace",
                                 "--no-default-features", "--features", peer),
                                (("RUSTDOCFLAGS", "-D warnings"),)) for peer in BACKENDS)
    return (*checks, *docs)


def refresh_cache(lane: str, backend: str | None = None) -> None:
    """Run shared cache refresh commands, inheriting the lane's Rust settings."""
    for command in refresh_commands(lane, backend):
        environment = dict(os.environ)
        environment.update(command.environment)
        subprocess.run(command.argv, env=environment, check=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("evidence", "aggregate"):
        commands.add_parser(name).add_argument("workflow", choices=WORKFLOWS)
    refresh = commands.add_parser("refresh")
    refresh.add_argument("lane", choices=CACHE_LANES)
    refresh.add_argument("--backend", choices=BACKENDS)
    args = parser.parse_args()
    if args.command == "refresh":
        refresh_cache(args.lane, args.backend)
        return 0
    workflow = WORKFLOWS[args.workflow]
    if args.command == "aggregate":
        needs = json.loads(os.environ["NEEDS"])
        results = {name: needs[job]["result"] for name, job in zip(workflow.lanes, workflow.needs)}
        outputs = needs["changes"]["outputs"]
        own_gate = {name: (needs[job]["result"], outputs.get(flag, "")) for name, job, flag in workflow.gated}
        passed = aggregate(needs["changes"]["result"], outputs.get("run", ""),
                           outputs.get("reuse", ""), results, workflow.cached, os.environ["EVENT_NAME"], own_gate)
        print(json.dumps({"changes": needs["changes"], "lanes": results, "own_gate": own_gate, "passed": passed}))
        return 0 if passed else 1
    tree = git_tree("HEAD")
    evidence = None
    try:
        api = GitHub(os.environ["GITHUB_REPOSITORY"], os.environ["GH_TOKEN"])
        evidence = find_evidence(
            api, workflow, json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text()),
            os.environ["GITHUB_EVENT_NAME"], tree, int(os.environ["GITHUB_REPOSITORY_ID"]),
            int(os.environ["GITHUB_RUN_ID"]))
    except (URLError, OSError, ValueError, KeyError, TypeError) as error:
        print(f"No applicable complete evidence: {error}; running normal CI")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"source_run={evidence or ''}\ntree={tree}\nreuse={'true' if evidence else 'false'}\n")
    reason = (f"Reusing full PR run {evidence} for exact tree `{tree}`. "
              "Original verification is at most 24 hours old. "
              "Verification steps skip; main cache-saving lanes still compile."
              if evidence else f"No trusted full PR run for tree `{tree}`; normal path gates apply.")
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
        summary.write(reason + "\n")
    print(reason)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
