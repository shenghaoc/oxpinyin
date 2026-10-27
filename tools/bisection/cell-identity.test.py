#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regressions for run-all.py's target-directory identity check."""
import json
import os
from pathlib import Path
import runpy
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

run_all = runpy.run_path(str(Path(__file__).with_name('run-all.py')))
ROOT = Path(__file__).resolve().parents[2]


class IdentityCheckTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.so = Path(self.scratch.name) / 'libpinyin_capi.so'
        self.so.write_bytes(b'x' * 10)
        stat = self.so.stat()
        self.identity = dict(schema=1, worktree=str(ROOT), cell='bdb', profile='dev',
                             artifacts={'pinyin-capi': dict(path=str(self.so), size=stat.st_size,
                                                            mtime_ns=stat.st_mtime_ns)})

    def tearDown(self):
        self.scratch.cleanup()

    def check(self, identity=..., needed=None, cell='bdb', root=ROOT):
        identity = self.identity if identity is ... else identity
        needed = {'pinyin-capi': str(self.so)} if needed is None else needed
        run_all['check_identity'](identity, root, cell, needed)

    def test_own_unchanged_artifacts_pass(self):
        self.check()
        self.check(needed={})

    def test_missing_record_is_a_hard_error(self):
        with self.assertRaisesRegex(ValueError, 'no artifact identity'):
            self.check(identity=None)

    def test_another_owner_cell_or_profile_is_a_hard_error(self):
        with self.assertRaisesRegex(ValueError, 'identity mismatch'):
            self.check(identity=dict(self.identity, worktree='/elsewhere'))
        with self.assertRaisesRegex(ValueError, 'identity mismatch'):
            self.check(cell='kc')
        with self.assertRaisesRegex(ValueError, 'identity mismatch'):
            self.check(identity=dict(self.identity, profile='release'))

    def test_an_artifact_changed_after_the_record_is_mixed(self):
        os.utime(self.so, ns=(self.so.stat().st_atime_ns, self.so.stat().st_mtime_ns + 1))
        with self.assertRaisesRegex(ValueError, 'changed after the recorded build'):
            self.check()

    def test_an_unstaged_or_missing_artifact_is_a_hard_error(self):
        with self.assertRaisesRegex(ValueError, 'was not staged'):
            self.check(needed={'zhuyin-capi': '/nowhere/libzhuyin_capi.so'})
        self.so.unlink()
        with self.assertRaisesRegex(ValueError, 'is missing'):
            self.check()

    def test_needed_artifacts_follow_the_declarations(self):
        env = dict(OXPINYIN_CAPI_SO='/t/libpinyin_capi.so', OXPINYIN_ZHUYIN_SO='/t/libzhuyin_capi.so',
                   OXPINYIN_DICTOOL='/t/oxpinyin-dictool', OXPINYIN_USER_RT_TEST='/t/user-dir-round-trip-test')
        rows = [dict(name='import-diff', artifacts=['pinyin-capi', 'dictool'], command=['{capi}']),
                dict(name='pc-version', artifacts=[], command=['{capi}', '{zhuyin}']),
                dict(name='user-dir-round-trip', artifacts=['pinyin-capi', 'user-rt-test'], command=[])]
        self.assertEqual(run_all['needed_artifacts'](rows, env),
                         {'pinyin-capi': '/t/libpinyin_capi.so', 'dictool': '/t/oxpinyin-dictool',
                          'user-rt-test': '/t/user-dir-round-trip-test'})
        self.assertEqual(run_all['needed_artifacts']([dict(name='pc-version', artifacts=[])], env), {})

    def test_identity_of_reads_the_record_or_none(self):
        self.assertIsNone(run_all['identity_of'](self.scratch.name))
        (Path(self.scratch.name) / 'oxpinyin-cell-identity.json').write_text('{"schema": 1}')
        self.assertEqual(run_all['identity_of'](self.scratch.name), {'schema': 1})


class ClaimTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        # Physical, like the owner cell-artifacts.sh records (pwd -P): the macOS
        # default temporary directory is under the /var -> /private/var link.
        self.root = Path(self.scratch.name).resolve()
        self.target = self.root / 'target'
        self.entered = self.root / 'entered'
        self.entered.mkdir()
        self.release = self.root / 'release'
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        cargo = self.bin / 'cargo'
        cargo.write_text('''#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
import json, os, pathlib, sys, time
target = pathlib.Path(os.environ['CARGO_TARGET_DIR'])
owner = pathlib.Path(sys.argv[sys.argv.index('--manifest-path') + 1]).resolve().parent
claim = json.loads((target / 'oxpinyin-cell-identity.json').read_text())
assert claim['worktree'] == str(owner)
assert (claim['cell'], claim['feature'], claim['profile'], claim['opt_level']) == ('bdb', 'bdb', 'dev', '1')
assert os.getsid(0) != int(os.environ['STUB_CALLER_SID']), "cargo shares its caller's session"
(pathlib.Path(os.environ['STUB_ENTERED']) / str(os.getpid())).write_text(str(owner))
while not pathlib.Path(os.environ['STUB_RELEASE']).exists():
    time.sleep(0.01)
if os.environ.get('STUB_FAIL') == '1':
    sys.exit(17)
(target / 'debug').mkdir(exist_ok=True)
(target / 'debug/libpinyin_capi.so').write_bytes(b'stub library')
''')
        cargo.chmod(0o755)
        # The build goes through slot.run_command in its own process, which
        # starts cargo in a new session it supervises (the stub checks). The
        # compatibility image has no just, so calling it fails.
        just = self.bin / 'just'
        just.write_text('#!/bin/sh\necho "just must not run: $*" >&2\nexit 99\n')
        just.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
                        CARGO_TARGET_DIR=str(self.target), PINYIN_ORACLE_DBM='bdb',
                        OXPINYIN_LOCK_DIR=str(self.root / 'locks'), OXPINYIN_HEAVY_SLOTS='1',
                        STUB_ENTERED=str(self.entered), STUB_RELEASE=str(self.release))
        self.env.pop('OXPINYIN_HEAVY_SLOT_HELD', None)

    def command(self, owner, artifacts='pinyin-capi'):
        # Real admission in a private lock directory, reading a fixed snapshot so
        # that a busy host cannot hold the stub build back.
        resources = dict(available_mib=12000, total_mib=16384, cpus=4, cargo_cpus=4, memory_limit_mib=None, notes=[])
        program = ('import os, sys; from pathlib import Path; '
                   f'sys.path.insert(0, {str(ROOT)!r}); '
                   'os.environ["STUB_CALLER_SID"] = str(os.getsid(0)); '
                   f'from tools.dev import host; host.resource_snapshot = lambda: {resources!r}; '
                   'from tools.dev.cell import main; '
                   'raise SystemExit(main(["--artifacts", sys.argv[2]], root=Path(sys.argv[1])))')
        return [sys.executable, '-X', 'lazy_imports=normal', '-c', program, str(owner), artifacts]

    def test_concurrent_owners_only_one_reaches_cargo(self):
        owners = []
        for name in ('first', 'second'):
            owner = self.root / name
            for relative in ('tools/oracle/build-oracle.sh', '.python-version'):
                destination = owner / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / relative, destination)
            owners.append(owner)
        processes = []
        try:
            for owner in owners:
                processes.append(subprocess.Popen(self.command(owner), env=dict(self.env, STUB_FAIL='1'),
                                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                                  start_new_session=True))
            deadline = time.monotonic() + 5
            while not any(process.poll() is not None for process in processes) and time.monotonic() < deadline:
                time.sleep(0.01)
            rejected = [index for index, process in enumerate(processes) if process.poll() is not None]
            self.assertEqual(len(rejected), 1, 'One owner must fail before the build can finish')
            loser = rejected[0]
            _, error = processes[loser].communicate(timeout=5)
            self.assertEqual(processes[loser].returncode, 3, error)
            self.assertIn('belongs to', error)
            self.release.touch()
            _, error = processes[1 - loser].communicate(timeout=5)
            self.assertEqual(processes[1 - loser].returncode, 17, error)
            entries = list(self.entered.iterdir())
            self.assertEqual(len(entries), 1)
            self.assertEqual(entries[0].read_text(), str(owners[1 - loser]))
        finally:
            self.release.touch()
            for process in processes:
                try:
                    process.communicate(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.communicate(timeout=5)

    def test_empty_artifacts_leave_missing_target_absent(self):
        result = subprocess.run(self.command(ROOT, ''), env=self.env, capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.target.exists())
        self.assertFalse((self.target / 'oxpinyin-cell-identity.json').exists())
        self.assertEqual(list(self.entered.iterdir()), [])

    def test_malformed_inherited_marker_fails_before_cargo_without_traceback(self):
        result = subprocess.run(self.command(ROOT), env=dict(self.env, OXPINYIN_HEAVY_SLOT_HELD='{'),
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 1)
        self.assertIn('Invalid OXPINYIN_HEAVY_SLOT_HELD', result.stderr)
        self.assertNotIn('Traceback', result.stderr)
        self.assertEqual(list(self.entered.iterdir()), [])

    def test_unreadable_identity_stops_before_cargo_without_traceback(self):
        self.target.mkdir()
        identity = self.target / 'oxpinyin-cell-identity.json'
        for content in (b'', b'{', b'[]', b'null', b'\xff'):
            with self.subTest(content=content):
                identity.write_bytes(content)
                result = subprocess.run(self.command(ROOT), env=self.env,
                                        capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('unreadable identity record', result.stderr)
                self.assertIn(str(identity), result.stderr)
                self.assertNotIn('Traceback', result.stderr)
                self.assertEqual(identity.read_bytes(), content)
                self.assertEqual(list(self.entered.iterdir()), [])
                self.assertEqual(list(self.target.iterdir()), [identity])

    def test_symlinked_build_records_physical_owner_and_passes_identity_check(self):
        link = self.root / 'checkout'
        link.symlink_to(ROOT, target_is_directory=True)
        self.release.touch()
        result = subprocess.run(self.command(link), env=self.env, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        identity = run_all['identity_of'](self.target)
        self.assertEqual(identity['worktree'], str(ROOT))
        self.assertEqual(len(list(self.entered.iterdir())), 1)
        run_all['check_identity'](identity, ROOT, 'bdb',
                                  {'pinyin-capi': str(self.target / 'debug/libpinyin_capi.so')})


if __name__ == '__main__':
    unittest.main()
