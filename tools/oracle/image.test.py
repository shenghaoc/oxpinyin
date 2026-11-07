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
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.oracle import image as images


class PerformanceImageTests(unittest.TestCase):
    def test_native_platform_is_explicit_for_build_and_run(self):
        with mock.patch.dict(os.environ, {}, clear=True), mock.patch.object(images.platform, 'machine', return_value='arm64'), \
             mock.patch.object(images, 'run', return_value=0) as run, \
             mock.patch.object(images, 'text', return_value='/checkout/.git'):
            self.assertEqual(images.performance(['--engine', 'container', '--', 'true']), 0)
        build, launch = [call.args[0] for call in run.call_args_list]
        self.assertEqual(build[build.index('--platform') + 1], 'linux/arm64')
        self.assertEqual(build[build.index('--memory') + 1], '6G')
        self.assertIn('/checkout/.git:/checkout/.git:ro', launch)
        self.assertIn(f'{ROOT.resolve()}:/repo:ro', launch)
        self.assertIn('ORACLE_PLATFORM=linux/arm64', build)
        self.assertEqual(build[build.index('--target') + 1], 'performance')
        self.assertEqual(launch[launch.index('--platform') + 1], 'linux/arm64')
        self.assertEqual(launch[-1], 'true')

    def test_ci_and_forced_emulation_refuse_before_any_build(self):
        for env in ({'GITHUB_ACTIONS': 'true'}, {'ORACLE_PLATFORM': 'linux/amd64'},
                    {'DOCKER_DEFAULT_PLATFORM': 'linux/amd64'}, {'CONTAINER_DEFAULT_PLATFORM': 'linux/amd64'}):
            with self.subTest(env=env), mock.patch.dict(os.environ, env, clear=True), \
                 mock.patch.object(images.platform, 'machine', return_value='arm64'), \
                 mock.patch.object(images, 'run') as run, contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(images.main(['performance', '--engine', 'container', '--build-only']), 2)
                run.assert_not_called()

    def test_rosetta_refuses_even_when_the_interpreter_reports_amd64(self):
        with mock.patch.dict(os.environ, {}, clear=True), \
             mock.patch.object(images.platform, 'system', return_value='Darwin'), \
             mock.patch.object(images.platform, 'machine', return_value='x86_64'), \
             mock.patch.object(images.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'1\n', b'')) as capture, \
             mock.patch.object(images, 'run') as run, contextlib.redirect_stderr(io.StringIO()) as error:
            self.assertEqual(images.main(['performance', '--engine', 'container']), 2)
            self.assertIn('native Python', error.getvalue())
            capture.assert_called_once_with(['/usr/sbin/sysctl', '-n', 'sysctl.proc_translated'], check=False, capture_output=True)
            run.assert_not_called()

    def test_reproducibility_compares_two_builds_not_a_frozen_digest(self):
        from tools.dev.measure import recipes
        for mismatch in (False, True):
            with tempfile.TemporaryDirectory() as directory:
                builds = []
                def build(command, **kwargs):
                    prefix = Path(command[command.index('--prefix') + 1])
                    builds.append(prefix)
                    (prefix / 'lib').mkdir(parents=True)
                    for library in ('libpinyin', 'libzhuyin'):
                        (prefix / 'lib' / (library + '.so')).write_text(str(len(builds) if mismatch else 1))
                    return 0
                with mock.patch.object(recipes, 'require_performance_environment'), \
                     mock.patch.object(images, 'run', side_effect=build), \
                     contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    args = ['--cells', 'bdb', '--work-dir', directory]
                    if mismatch:
                        with self.assertRaises(images.Finished):
                            images.check_reproducible(args)
                    else:
                        self.assertEqual(images.check_reproducible(args), 0)
                self.assertEqual(len(builds), 2)
                self.assertEqual(builds[0], builds[1])  # configure embeds the install prefix

    def check_reproducible(self, directory, build):
        from tools.dev.measure import recipes
        with mock.patch.object(recipes, 'require_performance_environment'), \
             mock.patch.object(images, 'run', side_effect=build), \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return images.check_reproducible(['--cells', 'bdb', 'kc', '--work-dir', str(directory)])

    def test_reproducibility_failure_removes_only_the_failing_backends_paths(self):
        def builder(fail_at=None, differ=False):
            builds = []

            def build(command, **kwargs):
                builds.append(command)
                prefix = Path(command[command.index('--prefix') + 1])
                Path(command[command.index('--work-dir') + 1]).mkdir(parents=True)
                (prefix / 'lib').mkdir(parents=True)
                for library in ('libpinyin', 'libzhuyin'):
                    (prefix / 'lib' / (library + '.so')).write_text(str(len(builds)) if differ else 'same')
                if len(builds) == fail_at:
                    raise images.Finished(7)
                return 0
            return build

        for label, build, status, kept in (
            ('first build', builder(fail_at=1), 7, False),
            ('second build', builder(fail_at=2), 7, False),
            ('comparison', builder(differ=True), 1, False),
            ('next backend', builder(fail_at=3), 7, True),
        ):
            with self.subTest(failure=label), tempfile.TemporaryDirectory() as directory:
                work = Path(directory)
                with self.assertRaises(images.Finished) as raised:
                    self.check_reproducible(work, build)
                self.assertEqual(raised.exception.status, status, 'the original failure is re-raised')
                left = {path.relative_to(work).as_posix() for path in work.rglob('*.so')}
                passed = {f'bdb-{number}/libraries/{library}.so' for number in (1, 2) for library in ('libpinyin', 'libzhuyin')}
                self.assertEqual(left, passed if kept else set())
                self.assertFalse((work / 'bdb/prefix').exists() or (work / 'kc/prefix').exists()
                                 or (work / 'kc-1').exists() or (work / 'kc-2').exists())
                self.assertEqual((work / 'bdb-1').exists(), kept)

    def test_reproducibility_refusal_keeps_existing_paths_and_builds_nothing(self):
        for existing in ('bdb/prefix', 'bdb-1', 'bdb-2'):
            with self.subTest(existing=existing), tempfile.TemporaryDirectory() as directory:
                work = Path(directory)
                (work / existing / 'kept').mkdir(parents=True)
                build = mock.Mock(return_value=0)
                with self.assertRaises(images.Finished):
                    self.check_reproducible(work, build)
                build.assert_not_called()
                self.assertTrue((work / existing / 'kept').is_dir())

    def test_platform_override_is_not_an_option(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            images.performance(['--platform', 'linux/amd64'])


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
        self.assertEqual(images.platform_field(pin, 'linux/amd64', 'platform'), 'linux/amd64')
        self.assertEqual(images.platform_field(pin, 'linux/arm64', 'platform'), 'linux/arm64')
        self.assertEqual(images.platform_field(pin, 'linux/riscv64', 'platform'), '')
        fields = images.fields(pin)
        self.assertFalse(any(key.startswith(('libpinyin_sha256_', 'libzhuyin_sha256_', 'data_manifest_sha256_')) for key in fields))

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

        keys = ('platform', 'libpinyin_commit', 'base_image', 'absent_field', 'model_archive_sha256')
        for platform, suffix in (('linux/amd64', ''), ('linux/arm64', '_linux_arm64')):
            for key in keys:
                with self.subTest(platform=platform, key=key):
                    match = re.search('^' + re.escape(key + suffix) + '=(.*)$', text, re.M)
                    expected = (0, match.group(1) + '\n' if match else '')
                    self.assertEqual(current(platform, key), expected)
        self.assertEqual(current('linux/amd64', 'platform'), (0, 'linux/amd64\n'))
        self.assertEqual(current('linux/arm64', 'platform'), (0, 'linux/arm64\n'))
        self.assertEqual(current('linux/arm64', 'libpinyin_commit'), (0, ''))
        status, output = current('linux/riscv64', 'platform')
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
