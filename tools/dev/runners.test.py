#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Portable runner policy tests with explicit command and host observations."""
from contextlib import suppress
from pathlib import Path
import os
import json
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev.runner_recipes import pinyin_needed, pinyin_resolved


class ElfObservationTests(unittest.TestCase):
    def test_elf_observations_are_parsed_without_a_linux_host(self):
        self.assertEqual(pinyin_needed(' (NEEDED) Shared library: [libpinyin.so.15]\n Shared library: [libc.so.6]'), 'libpinyin.so.15')
        self.assertEqual(pinyin_resolved('libpinyin.so.15 => /build/lib/libpinyin.so.15 (0x1)'), '/build/lib/libpinyin.so.15')
        self.assertEqual(pinyin_needed('Shared library: [libpinyin_capi.so]'), 'libpinyin_capi.so')


def live_descendants(pid, proc=Path('/proc')):
    """Every executing descendant of PID as (pid, process group) pairs, from one read of /proc.

    Zombies are left out: they hold no pipes and no work. One read is one
    snapshot, so a caller tearing a tree down repeats it until nothing is left.
    """
    children = {}
    for entry in proc.iterdir():
        if not entry.name.isdigit():
            continue
        try:
            fields = (entry / 'stat').read_text().rsplit(')', 1)[1].split()
        except (FileNotFoundError, ProcessLookupError):
            continue
        if fields[0] not in ('Z', 'X'):
            children.setdefault(int(fields[1]), []).append((int(entry.name), int(fields[2])))
    found, pending = [], [pid]
    while pending:
        for member in children.get(pending.pop(), ()):
            found.append(member)
            pending.append(member[0])
    return found


# Ceiling, in seconds, on each wait the ownership exercise makes for an explicit
# condition: the driver's ready marker, the signalled wrapper's exit, the killed
# driver's disappearance. Unloaded, each condition holds within a fraction of a
# second. The ceiling only bounds a starved host, and reaching it fails loudly
# with the condition and the wrapper's output; it measures nothing.
OWNERSHIP_CEILING = 60


class LinuxOwnershipTests(unittest.TestCase):
    def live(self, pid):
        try:
            return Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0] != 'Z'
        except FileNotFoundError:
            return False

    def until(self, condition, what, process=None):
        """Poll until CONDITION holds; fail naming WHAT if PROCESS ends first or the ceiling passes."""
        deadline = time.monotonic() + OWNERSHIP_CEILING
        while not condition():
            if process is not None and process.poll() is not None:
                self.fail(f'{what}: the wrapper exited with status {process.returncode} first')
            if time.monotonic() >= deadline:
                self.fail(f'{what} within {OWNERSHIP_CEILING}s')
            time.sleep(0.02)

    def stop(self, process, record, pids):
        """Kill every session the exercise started, reap the outer wrapper and return its output.

        The inner wrapper leads its own session and shares the outer wrapper's
        stdout and stderr pipes; the driver leads another. Killing the outer alone
        leaves the pipes open until the driver's sleep ends, so every session has
        to be gone before the wait for the outer's output. The tree is read from
        /proc while the outer is still alive, since a killed parent's children are
        reparented away from it, and re-read until nothing it started executes.
        No record is needed for this, so a start that never published one is torn
        down the same way. The driver the SIGKILL case orphans is no longer a
        descendant; the group `children` recorded covers it.
        """
        own = os.getpgrp()

        def kill(pid, group):
            with suppress(ProcessLookupError):
                if group == own:
                    os.kill(pid, signal.SIGKILL)
                else:
                    os.killpg(group, signal.SIGKILL)

        for group in pids[:1]:
            kill(group, group)
        deadline = time.monotonic() + OWNERSHIP_CEILING
        while process.poll() is None and (members := live_descendants(process.pid)):
            for pid, group in members:
                kill(pid, group)
            if time.monotonic() >= deadline:
                raise AssertionError(f'the wrapper tree still executed after {OWNERSHIP_CEILING}s: {members}')
            time.sleep(0.02)
        if process.poll() is None:
            process.kill()
        output = process.communicate(timeout=OWNERSHIP_CEILING)
        if record.exists():
            shutil.rmtree(json.loads(record.read_text())['work'], ignore_errors=True)
        return output

    def exercise(self, terminate):
        # Each record is renamed into place complete, so its existence is readiness.
        inner = '''
from contextlib import ExitStack
from pathlib import Path
import json, os, sys
from tools.dev import pool, slot
from tools.dev.cell import Settings
from tools.dev.drivers import DriverServices
from tools.dev.runners import Context
with slot.cancellation_handlers(), ExitStack() as stack:
    settings = Settings.read(os.environ)
    context = pool.UnitContext('fixture', slot.CANCELLED, environment=dict(settings.environment))
    services = DriverServices(settings, context)
    work = Context(settings, services, stack, Path.cwd(), 'fixture').scratch()
    Path(sys.argv[1] + '.part').write_text(json.dumps(dict(pid=os.getpid(), work=str(work))))
    os.replace(sys.argv[1] + '.part', sys.argv[1])
    code = 'import json,os,subprocess,sys,time; from pathlib import Path; child=subprocess.Popen([sys.executable,"-c","import time; time.sleep(60)"]); Path(sys.argv[1]+".part").write_text(json.dumps([os.getpid(),child.pid])); os.replace(sys.argv[1]+".part",sys.argv[1]); time.sleep(60)'
    try:
        services.command([sys.executable, '-c', code, sys.argv[2]])
    except pool.UnitStopped:
        result = slot.cancellation_status()
    else:
        result = 0
raise SystemExit(result)
'''
        outer = ("import sys; from tools.dev import slot; "
                 f"code={inner!r}; "
                 "result=slot.run_command([sys.executable,'-X','utf8','-c',code,*sys.argv[1:]]); "
                 "raise SystemExit(128-result if result<0 else result)")
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            record, children = directory / 'record', directory / 'children'
            env = dict(os.environ)
            process = subprocess.Popen([sys.executable, '-X', 'utf8', '-c', outer, str(record), str(children)],
                                       cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            pids, failure = [], None
            try:
                self.until(children.exists, 'the driver did not start', process)
                pids = json.loads(children.read_text())
                owner = json.loads(record.read_text())
                os.kill(owner['pid'], terminate)
                try:
                    process.communicate(timeout=OWNERSHIP_CEILING)
                except subprocess.TimeoutExpired:
                    self.fail(f'the signalled wrapper did not exit within {OWNERSHIP_CEILING}s')
                self.assertEqual(process.returncode, 128 + terminate)
                self.assertFalse(Path(owner['work']).exists(), 'cancelled runner kept its private scratch')
                self.assertFalse(any(self.live(pid) for pid in pids), 'cancelled runner kept a live descendant')
            except BaseException as error:
                failure = error
                raise
            finally:
                _, stderr = self.stop(process, record, pids)
                if failure is not None:
                    failure.add_note('wrapper stderr:\n' + stderr.decode(errors='replace'))

    if sys.platform.startswith('linux'):
        def test_sigterm_reaps_descendants_and_removes_scratch(self):
            self.exercise(signal.SIGTERM)


if __name__ == '__main__':
    unittest.main()
