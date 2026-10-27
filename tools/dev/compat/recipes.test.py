#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Oracle controller fixtures and optional preserved-base entrypoint comparisons."""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev import pool, slot
from tools.dev.compat import campaigns, commands, fixtures, identity, images, launcher, process
from tools.dev.runners import Finished


class RecipeTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)
        self.base = Path(os.environ['U3_BASE']).resolve() if os.environ.get('U3_BASE') else None
        self.env = dict(os.environ, UV_NO_CACHE='1')
        for key in tuple(self.env):
            if key in launcher.boundary.STATE_SETTINGS or key.startswith(('OXPINYIN_', 'PINYIN_', 'ORACLE_', 'UV_PYTHON_')):
                self.env.pop(key)
        # A forwarding uv exercises the actual modules without uv's own version diagnostics.
        uv = self.directory / 'uv'
        uv.write_text(f'#!{sys.executable}\nimport os,sys\na=sys.argv[1:]\ni=a.index("python"); os.execv(sys.executable,[sys.executable,*a[i+1:]])\n')
        uv.chmod(0o755)
        self.env['PATH'] = str(self.directory) + os.pathsep + self.env['PATH']

    def compare(self, script, recipe, args, *, streams=True):
        current = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev', 'compat', *recipe, *args],
                                 cwd=ROOT, env=self.env, capture_output=True, timeout=20)
        if self.base:
            old = subprocess.run(['bash', str(self.base / script), *args], cwd=ROOT, env=self.env, capture_output=True, timeout=20)
            self.assertEqual(current.returncode, old.returncode)
            if streams:
                def normalized(data):
                    return data.replace(str(self.base).encode(), b'<root>').replace(str(ROOT).encode(), b'<root>')
                self.assertEqual(normalized(current.stdout), normalized(old.stdout))
                self.assertEqual(normalized(current.stderr), normalized(old.stderr))
        return current

    def test_compat_first_pass_invalid_arguments_match_the_base(self):
        for args, code in (([], 2), (['--cell', 'invalid', '--list'], 2), (['--jobs', '0', '--list'], 2),
                           (['--memory-mib', '00', '--list'], 2), (['--runners', ', ,'], 2),
                           (['--unknown'], 2), (['--runtime'], 2), (['--memory-smoke', '--runners', 'bisect'], 1),
                           (['--help'], 0)):
            with self.subTest(args=args):
                self.assertEqual(self.compare('tools/oracle/compat.sh', [], args).returncode, code)

    def test_developer_input_errors_match_the_base(self):
        for args in ([], ['--unknown'], ['--libpinyin', '/absent']):
            self.assertEqual(self.compare('tools/oracle/run-differentials.sh', ['differentials'], args).returncode, 2)
        done = self.compare('tools/oracle/user-dir-round-trip.sh', ['user-round-trip'], ['/missing-prefix'])
        self.assertEqual(done.returncode, 77)
        self.assertEqual(self.compare('tools/datagen/libpinyin-drop-in-differential.sh', ['drop-in'], ['invalid', str(self.directory)]).returncode, 2)

    def test_image_options_and_invalid_platform_match_the_base(self):
        for script, recipe in (('tools/oracle/verify-oracle-image.sh', 'verify-image'),
                               ('tools/oracle/check-reproducible.sh', 'reproducible')):
            for args in (['--unknown'], ['--platform', 'linux/riscv64', 'image'] if recipe == 'verify-image'
                         else ['--platform', 'linux/riscv64']):
                self.assertEqual(self.compare(script, [recipe], args).returncode, 2)
        self.assertEqual(self.compare('tools/oracle/verify-oracle-image.sh', ['verify-image'], []).returncode, 2)

    def test_reference_tag_keeps_its_original_input_order_and_digest(self):
        result = self.compare('tools/oracle/image-tag.sh', ['image-tag'], [])
        self.assertEqual(result.stdout.decode().strip(), images.image_tag())
        before = images.image_tag()
        copied = self.directory / 'tree'
        for path in ('tools/oracle/Dockerfile.oracle','tools/oracle/build-oracle.sh','tools/oracle/version-manifest.json',
                     'tools/oracle/versions.py','tools/tkrzw/distro-probe.sh','tools/oracle/oracle-pin.txt'):
            dest = copied / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / path, dest)
        self.assertEqual(images.image_tag(copied), before)
        (copied / 'tools/oracle/version-manifest.json').write_bytes(b'changed input')
        self.assertNotEqual(images.image_tag(copied), before)

    def test_platform_pin_selection_moves_the_shell_regressions(self):
        pin = ROOT / 'tools/oracle/oracle-pin.txt'
        amd = images.platform_field(pin, 'linux/amd64', 'libpinyin_sha256_bdb')
        arm = images.platform_field(pin, 'linux/arm64', 'libpinyin_sha256_bdb')
        self.assertRegex(amd, '^[0-9a-f]{64}$')
        self.assertRegex(arm, '^[0-9a-f]{64}$')
        self.assertNotEqual(amd, arm)
        self.assertEqual(images.platform_field(pin, 'linux/riscv64', 'libpinyin_sha256_bdb'), '')
        for cell in ('bdb', 'kc', 'tkrzw'):
            for field in ('libpinyin_sha256_', 'libzhuyin_sha256_', 'data_manifest_sha256_'):
                self.assertRegex(images.platform_field(pin, 'linux/arm64', field + cell), '^[0-9a-f]{64}$')

    def test_image_identity_is_the_original_byte_stream(self):
        digest = hashlib.sha256()
        for path in ('tools/oracle/Dockerfile.compat', 'rust-toolchain.toml', '.python-version', 'tools/install-python.sh', 'tools/lib/bash-floor.sh'):
            digest.update((ROOT / path).read_bytes())
        digest.update(b'reference:no-custom-ca:linux/amd64')
        self.assertEqual(launcher.image_identity(ROOT, 'reference', 'no-custom-ca', 'linux/amd64'), digest.hexdigest()[:16])

    def test_image_copy_failure_keeps_child_status_and_removes_only_its_container(self):
        engine = self.directory / 'docker'
        log = self.directory / 'calls'
        engine.write_text(f'''#!{sys.executable}
import json,pathlib,sys
with pathlib.Path({str(log)!r}).open('a') as f: f.write(json.dumps(sys.argv[1:])+"\\n")
if sys.argv[1]=='create': print('owned-id')
if sys.argv[1]=='cp': sys.exit(19)
''')
        engine.chmod(0o755)
        args = ['--engine', str(engine), 'image']
        self.assertEqual(self.compare('tools/oracle/verify-oracle-image.sh', ['verify-image'], args).returncode, 19)
        rows = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertTrue(all(row == ['rm', '-f', 'owned-id'] for row in rows if row[0] == 'rm'))

    def test_capture_refuses_missing_oracle_without_writing_outputs(self):
        output = self.directory / 'output'
        done = self.compare('tools/capture/run-capture.sh', ['capture'], [str(self.directory), str(output)])
        self.assertEqual(done.returncode, 1)
        self.assertFalse(output.exists())

    def test_capture_rejects_wrong_schema_and_never_regenerates_fixtures(self):
        prefix = self.directory / 'prefix'
        for name in ('include/libpinyin-2.11.92/pinyin.h', 'lib/libpinyin.so.15.0.0', 'oracle-data.sha256'):
            path = prefix / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.touch()
        (prefix / 'lib/libpinyin/data').mkdir(parents=True)
        (prefix / 'oracle-pin.txt').write_text('schema=wrong\n')
        output = self.directory / 'output'
        done = self.compare('tools/capture/run-capture.sh', ['capture'], [str(prefix), str(output)])
        self.assertEqual(done.stderr, b'unsupported oracle manifest schema\n')
        self.assertFalse(output.exists())

    def test_heavy_launch_uses_the_existing_slot_and_launch_statuses(self):
        with mock.patch.object(process.slot, 'run_command', return_value=-15) as launch:
            self.assertEqual(process.run(['cargo', 'test'], heavy=True, check=False), 143)
        self.assertEqual(launch.call_count, 1)
        for error, expected in ((FileNotFoundError(2, 'missing'), 127), (PermissionError(13, 'denied'), 126)):
            with mock.patch.object(process.subprocess, 'Popen', side_effect=error), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(process.run(['command'], check=False), expected)

    def test_aggregate_suite_failure_keeps_later_suites_running(self):
        with mock.patch.object(campaigns, 'run', side_effect=[7, 0]) as child, contextlib.redirect_stdout(io.StringIO()) as output:
            failed = campaigns.run_suite('first', ['cargo'], env={})
            failed |= campaigns.run_suite('second', ['cargo'], env={})
        self.assertTrue(failed)
        self.assertEqual(child.call_count, 2)
        self.assertIn('FAILED: first', output.getvalue())
        self.assertNotIn('FAILED: second', output.getvalue())


if __name__ == '__main__':
    unittest.main()
