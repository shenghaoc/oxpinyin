#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""tools/oracle/image.py: the oracle image tag, pin fields, image verification and runtime selection."""
import contextlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.oracle import image as images


class ImageTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)
        self.env = dict(os.environ, UV_NO_CACHE='1')
        for key in tuple(self.env):
            if key.startswith(('OXPINYIN_', 'PINYIN_', 'ORACLE_', 'UV_PYTHON_')):
                self.env.pop(key)

    def invoke(self, recipe, args):
        program = ('import sys\nfrom tools.oracle import image\n'
                   'raise SystemExit(image.main(sys.argv[1:]))\n')
        return subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', program, *recipe, *args],
                              cwd=ROOT, env=self.env, capture_output=True, timeout=20)

    def test_image_options_and_invalid_platform_statuses(self):
        for args in (['--unknown'], ['--platform', 'linux/riscv64', 'image']):
            self.assertEqual(self.invoke(['verify-image'], args).returncode, 2)
        self.assertEqual(self.invoke(['verify-image'], []).returncode, 2)

    def test_reference_tag_keeps_its_original_input_order_and_digest(self):
        result = self.invoke(['image-tag'], [])
        self.assertEqual(result.stdout.decode().strip(), images.image_tag())
        before = images.image_tag()
        copied = self.directory / 'tree'
        for path in ('tools/oracle/Dockerfile.oracle', 'tools/oracle/build-oracle.sh', 'tools/oracle/version-manifest.json',
                     'tools/oracle/versions.py', 'tools/tkrzw/distro-probe.sh', 'tools/lib/bash-floor.sh', 'tools/oracle/oracle-pin.txt'):
            dest = copied / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / path, dest)
        self.assertEqual(images.image_tag(copied), before)
        floor = copied / 'tools/lib/bash-floor.sh'
        original_floor = floor.read_bytes()
        floor.write_bytes(b'changed floor')
        self.assertNotEqual(images.image_tag(copied), before)
        floor.write_bytes(original_floor)
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

    def test_pin_field_reads_use_platform_fields_without_fallback(self):
        # Platform fields do not fall back to unqualified fields; missing fields
        # print nothing and unsupported platforms fail silently.
        pin = ROOT / 'tools/oracle/oracle-pin.txt'
        text = pin.read_text()

        def current(platform, key):
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                status = images.main(['pin-field', '--platform', platform, key])
            return status, out.getvalue()

        keys = ('platform', 'libpinyin_commit', 'base_image', 'absent_field', 'libpinyin_sha256_bdb',
                'libzhuyin_sha256_tkrzw', 'data_manifest_sha256_kc')
        for platform, suffix in (('linux/amd64', ''), ('linux/arm64', '_linux_arm64')):
            for key in keys:
                with self.subTest(platform=platform, key=key):
                    match = re.search('^' + re.escape(key + suffix) + '=(.*)$', text, re.M)
                    expected = (0, match.group(1) + '\n' if match else '')
                    self.assertEqual(current(platform, key), expected)
        self.assertEqual(current('linux/amd64', 'platform'), (0, 'linux/amd64\n'))
        self.assertEqual(current('linux/arm64', 'platform'), (0, 'linux/arm64\n'))
        self.assertEqual(current('linux/arm64', 'libpinyin_commit'), (0, ''))
        status, output = current('linux/riscv64', 'libpinyin_sha256_bdb')
        self.assertEqual((status != 0, output), (True, ''))

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
        self.assertEqual(self.invoke(['verify-image'], args).returncode, 19)
        rows = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertEqual([row[0] for row in rows], ['create', 'cp', 'rm'])
        self.assertEqual(rows[0], ['create', '--pull=never', '--platform', 'linux/amd64', 'image', '/none'])
        self.assertEqual(rows[1][:2], ['cp', 'owned-id:/opt/pinyin-oracle'])
        self.assertEqual([row for row in rows if row[0] == 'rm'], [['rm', '-f', 'owned-id']])

    def test_runtime_preference_and_explicit_override_are_preserved(self):
        runtime = self.directory / 'docker'
        runtime.write_text('#!/bin/sh\necho "fake 1.0"\n')
        runtime.chmod(0o755)
        for kind in ('container', 'podman'):
            (self.directory / kind).symlink_to(runtime)
        uname = self.directory / 'uname'
        uname.write_text('#!/bin/sh\nprintf "%s\\n" "$SYNTHETIC_HOST"\n')
        uname.chmod(0o755)
        for platform, override, expected in (('Darwin', '', 'container'), ('Linux', '', 'podman'),
                                             ('Darwin', 'docker', 'docker')):
            env = dict(self.env, SYNTHETIC_HOST=platform, OXPINYIN_CONTAINER=override,
                       PATH=str(self.directory) + os.pathsep + os.environ['PATH'])
            result = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c',
                                     'from tools.oracle.image import select_runtime; print("selected:" + select_runtime())'],
                                    cwd=ROOT, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('selected:' + expected, result.stdout)


if __name__ == '__main__':
    unittest.main()
