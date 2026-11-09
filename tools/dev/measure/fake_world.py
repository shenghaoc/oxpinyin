"""A world without tools for the measurement recipes' tests (#1290).

Every seam bench.py gives a recipe (capture, run, the libtkrzw check and
the report scripts) is replaced by a fake that writes an
ordered event log, so a test can pin what a runner did: its counts and banners, the order of arms
and modes, the files it leaves and the statuses it stops with. Nothing needs taskset, binutils,
cargo or /proc, so the tests that use it run on macOS and Linux. Not a test itself: the harness
discovers only *.test.py.
"""
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from tools.dev import pool, runners
from tools.dev.measure import bench, identity


class Stream(io.TextIOWrapper):
    """A text stream with a byte buffer behind it, as the console has: runners.write needs .buffer."""

    def __init__(self):
        super().__init__(io.BytesIO(), encoding='utf-8', newline='\n', write_through=True)

    def text(self) -> str:
        self.flush()
        return self.buffer.getvalue().decode()


class World(unittest.TestCase):
    """Replace every seam a recipe reaches its tools through, and log what it did in order."""

    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory(prefix='valgrind-test-'))).resolve()
        from . import valgrind
        scratch = self.root / 'scratch'
        scratch.mkdir()
        self.enterContext(mock.patch.object(valgrind, 'SCRATCH', scratch))
        self.log = []
        self.worker_status = {}        # nth worker launch (1-based) -> exit status
        self.workers = 0
        fakes = dict(capture=self.fake_capture, run=self.fake_run, check=self.fake_check,
                     report=self.fake_report, utc_now=lambda: '2026-10-27T00:00:00Z')
        for name, fake in fakes.items():
            self.enterContext(mock.patch.object(bench, name, fake))
        self.enterContext(mock.patch('shutil.which', lambda name, path=None: '/usr/bin/taskset' if name == 'taskset' else None))
        self.enterContext(mock.patch.object(identity, 'loadavg', lambda proc=None, fields=None: (
            '0.1 0.2 0.3 1/2 3' if fields is None else '0.1 0.2 0.3')))
        for name, value in (('cpu_model', 'Fancy CPU'), ('memory_total', 'MemTotal: 1 kB'), ('processors', 4),
                            ('package_version', '1.0'),
                            ('pc_version', 'Version: 2.11.92'), ('first_output_line', 'tool 1.0'),
                            ('perf_event_paranoid', '2')):
            self.enterContext(mock.patch.object(identity, name, lambda *a, _value=value, **k: _value))

    # ── fakes ──────────────────────────────────────────────────────────────────────────────
    def fake_capture(self, command, *, env=None, cwd=None, merge_stderr=False):
        command = [str(part) for part in command]
        if command[:2] == ['taskset', '-c']:
            self.log.append(('taskset', command[2]))
            command = command[3:]
        if '--perf' in command:
            self.workers += 1
            self.log.append(('worker', env['PERF_BACKEND'], env['PERF_MODE'], env['PERF_CYCLES'],
                             env.get('PERF_REPEATS'), env.get('LD_LIBRARY_PATH'), command))
            row = {'backend': env['PERF_BACKEND'], 'mode': env['PERF_MODE'], 'n': self.workers}
            return pool.Completed(self.worker_status.get(self.workers, 0), (json.dumps(row) + '\n').encode(),
                                  b'worker diagnostics\n')
        self.log.append(('capture', command))
        return pool.Completed(0, b'', b'')

    def fake_run(self, command, *, env=None, cwd=None, stdout=None, check_status=True):
        command = [str(part) for part in command]
        self.log.append(('run', command))
        if '-o' in command:
            Path(command[command.index('-o') + 1]).write_bytes(b'\x7fELF harness')
        return 0

    def fake_check(self, cell, runner, side, obj, **kwargs):
        self.log.append(('check', cell, side, str(obj)))

    def fake_report(self, script, argv):
        self.log.append(('report', Path(script).name, argv[0] if argv else ''))
        return 0, f'REPORT {Path(script).name}\n'

    # ── driver ─────────────────────────────────────────────────────────────────────────────
    def execute(self, recipe, argv=(), env=None):
        out, err = Stream(), Stream()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            status = runners.guarded(lambda: recipe(list(argv), dict(env or {})))
        return status, out.text(), err.text()

    def kinds(self, kind):
        return [event for event in self.log if event[0] == kind]

    def workers_of(self, mode=None):
        return [event for event in self.kinds('worker') if mode in (None, event[2])]
