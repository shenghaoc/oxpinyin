"""Worker-process and run-output helpers of the contract driver (contract-diff.py).

Stop a worker's whole process group, list what a run left in the user
directories it made, read the case-pool size from --jobs or the environment,
and total the workers' phase timings. None reads the driver's per-run state;
the timings are a parameter. The runner itself (run_worker, run_case, case_jobs,
case_results and the cancellation flag) stays in contract-diff.py, where it
rebinds module state that its tests and the other tools patch.
"""
import argparse
import contextlib
import hashlib
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys


def terminate_group(proc):
    """Stop descendants even if the group leader exits during the grace period.

    proc must have been started as the process-group leader. Send SIGTERM,
    wait up to half a second for the leader, then send SIGKILL even if it
    exited and reap it. Ignore missing groups; other errors propagate."""
    with contextlib.suppress(ProcessLookupError):
        os.killpg(proc.pid, signal.SIGTERM)
    try:
        proc.wait(timeout=0.5)
    except subprocess.TimeoutExpired:
        pass
    with contextlib.suppress(ProcessLookupError):
        os.killpg(proc.pid, signal.SIGKILL)
    proc.wait()


def user_dir_listing(scratch, before):
    """What the run left in the user directories it created under `scratch`:
    every file by name, `user.conf` by text, chunk logs and their `.tmp`
    siblings by size and SHA-256, DBM containers by size. The directories
    are removed. The chunk hashes make a save's `.dbin` bytes and its
    partial writes exact comparisons; the DBM containers are value-level in
    this implementation, so only their size is held.

    before is the set of prior scratch entries. Scan new user-* directories
    in sorted order, flattening by filename so later names overwrite earlier
    ones. Replace user.conf newlines with '|'. Hash regular .tmp files and
    non-user_ .bin/.dbin files with their sizes; record only size for other files
    and None for non-files.
    Ignore directory-removal errors; file read/stat errors propagate."""
    listing = {}
    for entry in sorted(set(os.listdir(scratch)) - before):
        path = os.path.join(scratch, entry)
        if not entry.startswith('user-') or not os.path.isdir(path):
            continue
        for name in sorted(os.listdir(path)):
            file = os.path.join(path, name)
            if name == 'user.conf':
                listing[name] = Path(file).read_text().replace('\n', '|')
            elif os.path.isfile(file) and (name.endswith('.tmp') or
                                           (not name.startswith('user_') and
                                            name.endswith(('.bin', '.dbin')))):
                # Chunk logs and their partial `.tmp` siblings are compared
                # byte-for-byte; a replaced system chunk makes those bytes
                # the save's observable result.
                listing[name] = [os.path.getsize(file),
                                 hashlib.sha256(Path(file).read_bytes()).hexdigest()]
            elif os.path.isfile(file):
                # The DBM containers are value-level in this implementation;
                # the pin and subject agree on their size and records, not
                # their backend byte layout.
                listing[name] = [os.path.getsize(file)]
            else:
                listing[name] = None
        shutil.rmtree(path, ignore_errors=True)
    return listing


def positive_jobs(value):
    """Convert value with int and reject nonpositive worker counts.

    Nonpositive values raise ArgumentTypeError; conversion failures propagate
    from int rather than being wrapped here."""
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError('--jobs must be a positive integer')
    return number


def ordinary_jobs(parser, requested):
    """The case count of an ordinary run: --jobs, else the validated environment, else serial.

    Only ordinary contract runs read OXPINYIN_CONTRACT_JOBS, so a bad value
    in the environment cannot break --list-tags, --teardown or --per-call.

    Return requested unchanged when supplied. Otherwise validate the environment
    value through tools.dev.host and report invalid settings with parser.error;
    importing the validator may prepend the repository to sys.path. Return one
    when the setting is absent."""
    if requested is not None:
        return requested
    value = os.environ.get('OXPINYIN_CONTRACT_JOBS')
    if value is None:
        return 1
    root = str(Path(__file__).resolve().parents[2])
    if root not in sys.path:
        sys.path.insert(0, root)
    from tools.dev import host
    try:
        host.validate_settings({'OXPINYIN_CONTRACT_JOBS': value})
    except ValueError as error:
        parser.error(str(error))
    return int(value)


def timing_summary(timings, names, compare_seconds, total):
    """Seconds per phase summed over the workers the driver ran; pin and
    subject execution apart (the pin's worker is the first of each pair).

    timings is the driver's TIMINGS: one dict per worker, in completion order."""
    seconds = dict(startup=0.0, load=0.0, fixture=0.0, pin_execute=0.0, subject_execute=0.0,
                   teardown=0.0, compare=compare_seconds, total=total)
    for index, timing in enumerate(timings):
        for phase in ('startup', 'load', 'fixture', 'teardown'):
            seconds[phase] += timing.get(phase, 0.0)
        seconds[timing.get('side', 'pin' if index % 2 == 0 else 'subject') + '_execute'] += timing.get('execute', 0.0)
    return dict(cases=len(names), workers=len(timings),
                seconds={phase: round(value, 4) for phase, value in seconds.items()})
