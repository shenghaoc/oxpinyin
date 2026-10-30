"""A world without tools for the measurement recipes' tests (#1290).

Every seam bench.py gives a recipe (capture, run, heavy, the
libtkrzw check, the report scripts and the pool of builds) is replaced by a fake that writes an
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
from tools.dev.measure import bench, campaigns, identity


class Stream(io.TextIOWrapper):
    """A text stream with a byte buffer behind it, as the console has: runners.write needs .buffer."""

    def __init__(self):
        super().__init__(io.BytesIO(), encoding='utf-8', write_through=True)

    def text(self) -> str:
        self.flush()
        return self.buffer.getvalue().decode()


class World(unittest.TestCase):
    """Replace every seam a campaign reaches its tools through, and log what it did in order."""

    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory(prefix='campaigns-test-'))).resolve()
        self.log = []
        self.taskset = True
        self.worker_status = {}        # nth worker launch (1-based) -> exit status
        self.workers = 0
        self.builds = []
        self.reports = []
        self.report_status = {}
        fakes = dict(capture=self.fake_capture, run=self.fake_run, heavy=self.fake_heavy, check=self.fake_check,
                     report=self.fake_report, build_all=self.fake_build_all,
                     utc_now=lambda: '2026-10-27T00:00:00Z')
        for name, fake in fakes.items():
            self.enterContext(mock.patch.object(bench, name, fake))
        self.enterContext(mock.patch('shutil.which', lambda name, path=None: '/usr/bin/taskset' if (
            name == 'taskset' and self.taskset) else None))
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
        if command[0] == 'pkg-config':
            answers = {'--cflags': '-I/stage/usr/include \n', '--libs': '-L/stage/usr/lib -lpinyin\n',
                       '--variable=libdir': f'{self.root}/work/stage/usr/lib\n'}
            self.log.append(('pkg-config', command[1], env.get('PKG_CONFIG_PATH'), env.get('PKG_CONFIG_SYSROOT_DIR')))
            return pool.Completed(0, answers[command[1]].encode(), b'')
        self.log.append(('capture', command))
        return pool.Completed(0, b'', b'')

    def fake_run(self, command, *, env=None, cwd=None, stdout=None, check_status=True):
        command = [str(part) for part in command]
        self.log.append(('run', command))
        if '-o' in command:
            Path(command[command.index('-o') + 1]).write_bytes(b'\x7fELF harness')
        return 0

    def fake_heavy(self, command, *, env=None, stdout=None, check_status=True):
        command = [str(part) for part in command]
        self.log.append(('heavy', command))
        if command[1] == 'cinstall':
            destdir = Path(next(part for part in command if part.startswith('--destdir=')).split('=', 1)[1])
            (destdir / 'usr/lib/pkgconfig').mkdir(parents=True, exist_ok=True)
            (destdir / 'usr/lib/pkgconfig/libpinyin.pc').write_text('Name: libpinyin\n')
            (destdir / 'usr/lib/libpinyin.so').write_bytes(b'\x7fELF capi')
            (destdir / 'usr/share/oxpinyin').mkdir(parents=True, exist_ok=True)
            for name in campaigns.REQUIRED_DATA:
                (destdir / 'usr/share/oxpinyin' / name).write_bytes(b'data')
        if command[1] == 'build':
            (self.target / 'release').mkdir(parents=True, exist_ok=True)
            (self.target / 'release/libpinyin_capi.so').write_bytes(b'\x7fELF ' + command[command.index('--features') + 1].encode())
        return 0

    target = None

    def fake_check(self, cell, runner, side, obj, **kwargs):
        self.log.append(('check', cell, side, str(obj)))

    guard_env = None


    def fake_report(self, script, argv):
        self.reports.append((script, list(argv)))
        self.log.append(('report', Path(script).name, argv[0] if argv else ''))
        return self.report_status.get(Path(script).name, 0), f'REPORT {Path(script).name}\n'

    def fake_build_all(self, builds, label, *, env=None):
        class Context:
            environment = {}

            def launch(context, command, *, env, merge_stderr):
                command = [str(part) for part in command]
                self.builds.append(command)
                flag = '--destdir' if '--destdir' in command else '--target-dir'
                directory = Path(command[command.index(flag) + 1])
                if command[1] == 'cinstall':
                    (directory / 'usr/lib').mkdir(parents=True)
                    (directory / 'usr/lib/libpinyin.so.15.0.0').write_bytes(b'\x7fELF ' + command[command.index('--features') + 1].encode())
                else:
                    (directory / 'release').mkdir(parents=True)
                    (directory / 'release/libpinyin_capi.so').write_bytes(b'\x7fELF cargo')
                return pool.Completed(0, b'cargo output\n', b'')
        self.log.append(('builds', label, [name for name, _ in builds]))
        for name, function in builds:
            function(Context(), 2)

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
