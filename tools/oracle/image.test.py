#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""tools/oracle/image.py: the oracle image tag, pin fields, image verification and runtime selection."""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import types
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
                     'tools/oracle/versions.py', 'tools/lib/bash-floor.sh', 'tools/oracle/oracle-pin.txt'):
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


class WslcAdapterTests(unittest.TestCase):
    """The wslc translation of each kept command; no container runs."""
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)

    def test_a_captured_childs_stderr_is_forwarded(self):
        # wslc and fetch-contract-data write diagnostics to stderr while their stdout is captured.
        import tools.dev.process as process
        done = mock.Mock(stdout=b'out', stderr=b'diagnostic\n', status=0)
        stream = io.TextIOWrapper(io.BytesIO(), encoding='utf-8')
        with mock.patch.object(process.bench, 'capture', return_value=done), mock.patch.object(sys, 'stderr', stream):
            self.assertIs(process.capture(['child']), done)
            stream.flush()
            self.assertEqual(stream.buffer.getvalue(), b'diagnostic\n')

    def test_docker_and_podman_commands_pass_through_unchanged(self):
        for engine in ('docker', '/usr/bin/podman', 'wslc-like'):
            command = [engine, 'create', '--pull=never', '--platform', 'linux/amd64', 'image', '/none']
            self.assertEqual(images.wslc_command(command), command)
            self.assertEqual(images.wslc_command([engine, 'rm', '-f', 'id']), [engine, 'rm', '-f', 'id'])

    def test_wslc_drops_platform_and_renames_rm(self):
        for engine in ('wslc', 'C:/Program Files/WSL/wslc.exe', 'WSLC.EXE'):
            with self.subTest(engine=engine):
                self.assertEqual(images.wslc_command([engine, 'create', '--pull=never', '--platform', 'linux/amd64', 'image', '/none']),
                                 [engine, 'create', '--pull=never', 'image', '/none'])
                self.assertEqual(images.wslc_command([engine, 'pull', '--platform=linux/arm64', 'image']), [engine, 'pull', 'image'])
                self.assertEqual(images.wslc_command([engine, 'rm', '-f', 'id']), [engine, 'remove', '-f', 'id'])
                self.assertEqual(images.wslc_command([engine, 'run', '--rm', 'image', 'rm']), [engine, 'run', '--rm', 'image', 'rm'])

    def test_image_field_reads_wslc_json_and_docker_templates(self):
        calls = []

        def fake(command, **kwargs):
            calls.append(command)
            return mock.Mock(stdout=json.dumps([{'Architecture': 'arm64', 'Id': 'sha256:abc'}]).encode(), status=0)
        with mock.patch.object(images, 'capture', fake):
            self.assertEqual(images.image_field('wslc', 'Architecture', 'image'), 'arm64')
            self.assertEqual(images.image_field('wslc.exe', 'Id', 'image'), 'sha256:abc')
        self.assertEqual(calls, [['wslc', 'image', 'inspect', '--format', 'json', 'image'],
                                 ['wslc.exe', 'image', 'inspect', '--format', 'json', 'image']])
        with mock.patch.object(images, 'text', return_value='amd64') as text:
            self.assertEqual(images.image_field('docker', 'Architecture', 'image'), 'amd64')
        text.assert_called_once_with(['docker', 'image', 'inspect', '--format', '{{.Architecture}}', 'image'], env=None)

    def verify_through_export(self, removal_status=0):
        """verify-image on wslc over a synthetic export; answer status, calls, stderr and the removal mock."""
        calls, archive = [], self.directory / 'export.tar'
        source = self.directory / 'tree'
        for dbm in images.cell.CELLS:
            (source / 'opt/pinyin-oracle' / dbm / 'lib/libpinyin/data').mkdir(parents=True)
            for library in ('libpinyin', 'libzhuyin'):
                (source / 'opt/pinyin-oracle' / dbm / 'lib' / (library + '.so')).write_text(dbm + library)
        (source / 'etc').mkdir()
        (source / 'etc/passwd').write_text('outside the oracle\n')
        with tarfile.open(archive, 'w') as tar:
            tar.add(source / 'opt', 'opt')
            tar.add(source / 'etc', 'etc')

        def fake_capture(command, **kwargs):
            calls.append(command)
            return mock.Mock(stdout=b'owned-id\n', status=0)

        def fake_run(command, **kwargs):
            calls.append([str(part) for part in command])
            shutil.copyfile(archive, command[command.index('-o') + 1])
            return 0
        platform = images.host_platform() or 'linux/amd64'
        with (mock.patch.object(images, 'capture', fake_capture), mock.patch.object(images, 'run', fake_run),
              mock.patch.object(images, 'host_platform', return_value=platform),
              mock.patch.object(images.subprocess, 'run', return_value=mock.Mock(returncode=removal_status)) as remove,
              contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as error):
            status = images.verify_image(['--engine', 'wslc', '--platform', platform, 'image'])
        return status, calls, error.getvalue(), remove

    def test_wslc_verify_image_exports_instead_of_copying(self):
        status, calls, error, remove = self.verify_through_export()
        self.assertEqual(status, 1)
        self.assertIn('FAIL: bdb: stale pin/recipe', error)
        self.assertRegex(error, r'(?m)export/opt/pinyin-oracle/bdb/lib/libpinyin/data$')
        self.assertNotIn('passwd', error)
        self.assertNotIn('WARNING', error)
        self.assertEqual(calls[0], ['wslc', 'create', '--pull=never', 'image', '/none'])
        self.assertEqual(calls[1][:3], ['wslc', 'export', '-o'])
        self.assertEqual(calls[1][4:], ['owned-id'])
        self.assertEqual(remove.call_args.args[0], ['wslc', 'remove', '-f', 'owned-id'])
        self.assertIs(remove.call_args.kwargs['check'], False)

    def test_failed_container_removal_warns_with_its_id_and_keeps_the_status(self):
        status, _, error, remove = self.verify_through_export(removal_status=125)
        self.assertEqual(status, 1)
        remove.assert_called_once()
        self.assertIn('WARNING: could not remove container owned-id (wslc exited 125)', error)

    def test_container_is_wslcs_alias_only_on_windows(self):
        for host, wslc in (('Windows', True), ('Darwin', False), ('Linux', False)):
            with self.subTest(host=host), mock.patch.object(images, 'host_system', return_value=host):
                self.assertIs(images.is_wslc('container'), wslc)
                self.assertIs(images.is_wslc('C:/Program Files/WSL/container.exe'), wslc)
                self.assertIs(images.is_wslc('wslc.exe'), True)
        with mock.patch.object(images, 'host_system', return_value='Windows'):
            self.assertEqual(images.wslc_command(['container.exe', 'create', '--platform', 'linux/amd64', 'i', '/none']),
                             ['container.exe', 'create', 'i', '/none'])

    def test_verify_image_runs_only_the_host_architecture(self):
        calls = []

        def fake_capture(command, **kwargs):
            calls.append(command)
            return mock.Mock(stdout=b'cid\n', status=0)
        with (mock.patch.object(images, 'host_platform', return_value='linux/arm64'),
              mock.patch.object(images, 'capture', fake_capture),
              mock.patch.object(images, 'run', return_value=0), mock.patch.object(images, 'digest_file', return_value='0'),
              mock.patch.object(images, 'prefix_ok', return_value=True),
              mock.patch.object(images.subprocess, 'run', return_value=mock.Mock(returncode=0)),
              contextlib.redirect_stdout(io.StringIO())):
            self.assertEqual(images.verify_image(['--engine', 'docker', 'image']), 0)
        self.assertEqual(calls[0][calls[0].index('--platform') + 1], 'linux/arm64')
        for engine in ('docker', 'podman', 'wslc'):
            with (self.subTest(engine=engine), mock.patch.object(images, 'host_platform', return_value='linux/arm64'),
                  mock.patch.object(images, 'capture') as capture, mock.patch.object(images, 'run') as run,
                  contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(images.Finished) as raised):
                images.verify_image(['--engine', engine, '--platform', 'linux/amd64', 'image'])
            self.assertEqual(raised.exception.status, 2)
            self.assertIn('every image runs at the host architecture (linux/arm64), with no emulation', error.getvalue())
            capture.assert_not_called()
            run.assert_not_called()

    def test_wslc_verify_image_refuses_a_foreign_platform(self):
        with mock.patch.object(images, 'host_platform', return_value='linux/amd64'), mock.patch.object(images, 'capture') as capture, \
                contextlib.redirect_stderr(io.StringIO()), self.assertRaises(images.Finished) as raised:
            images.verify_image(['--engine', 'wslc', '--platform', 'linux/arm64', 'image'])
        self.assertEqual(raised.exception.status, 2)
        capture.assert_not_called()

    def build_calls(self, runtime, machine):
        calls = []
        env = dict(OXPINYIN_CAPTURE_RUNTIME=runtime, OXPINYIN_CAPTURE_BUILD_FLAGS='--network host',
                   PATH=str(self.directory))
        (self.directory / runtime).write_text('')
        (self.directory / runtime).chmod(0o755)
        architecture = images.ARCHITECTURES[machine]
        with (mock.patch.object(images.platform, 'machine', return_value=machine),
              mock.patch.object(images, 'image_tag', return_value='TAG'),
              mock.patch.object(images, 'run', lambda command, **kwargs: calls.append(command) or 0),
              mock.patch.object(images, 'image_field', return_value=architecture) as field,
              mock.patch.object(images, 'verify_image', return_value=0) as verify,
              mock.patch.object(images.shutil, 'which', return_value=str(self.directory / runtime)),
              contextlib.redirect_stdout(io.StringIO()) as output):
            self.assertEqual(images.build_image([], env=env), 0)
        return calls, field.call_args, verify.call_args, output.getvalue()

    def test_build_image_keeps_the_docker_command_and_translates_wslc(self):
        calls, field, verify, output = self.build_calls('docker', 'x86_64')
        image = 'ghcr.io/shenghaoc/oxpinyin-oracle:TAG'
        self.assertEqual(calls, [['docker', 'build', '--platform', 'linux/amd64', '--build-arg', 'ORACLE_PLATFORM=linux/amd64',
                                  '--network', 'host', '-f', 'tools/oracle/Dockerfile.oracle', '-t', image, '.']])
        self.assertEqual(field.args, ('docker', 'Architecture', image))
        self.assertEqual(verify.args[0], ['--engine', 'docker', '--platform', 'linux/amd64', image])
        self.assertEqual(output, image + '\n')
        calls, field, verify, output = self.build_calls('wslc', 'aarch64')
        self.assertEqual(calls, [['wslc', 'build', '--build-arg', 'ORACLE_PLATFORM=linux/arm64',
                                  '--network', 'host', '-f', 'tools/oracle/Dockerfile.oracle', '-t', image + '-arm64', '.']])
        self.assertEqual(verify.args[0], ['--engine', 'wslc', '--platform', 'linux/arm64', image + '-arm64'])

    def test_build_image_refuses_another_architecture_before_building(self):
        with (mock.patch.object(images.platform, 'machine', return_value='riscv64'),
              mock.patch.object(images.shutil, 'which', return_value='docker'),
              mock.patch.object(images, 'run') as run, contextlib.redirect_stderr(io.StringIO()),
              self.assertRaises(images.Finished) as raised):
            images.build_image([], env={})
        self.assertEqual(raised.exception.status, 2)
        run.assert_not_called()

    def test_wslc_is_selected_first_on_windows_and_reports_its_version_line(self):
        def fake(command, **kwargs):
            return mock.Mock(stdout=b'wslc 3.0.3.0\nCopyright (c) Microsoft Corporation.\n', status=0)
        printed = []
        with mock.patch.object(images.shutil, 'which', side_effect=lambda name, path=None: name), \
                mock.patch.object(images, 'capture', fake):
            selected = images.select_runtime(env={'PATH': ''}, host='MINGW64_NT-10.0', output=printed.append)
        self.assertEqual(selected, 'wslc')
        self.assertEqual(printed, ['Runtime: wslc; wslc 3.0.3.0'])

class CaptureLauncherTests(unittest.TestCase):
    """The commands `just capture-pin` builds for each runtime; no container runs."""
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.provision = Path(directory.name).resolve() / 'provision'
        (self.provision / 'model20/extracted').mkdir(parents=True)

    def launch(self, runtime, *, present=True, machine='x86_64', uid=(1000, 1001), argv=('1',)):
        calls = []

        def fake_run(command, **kwargs):
            calls.append([str(part) for part in command])
            return 0

        def field(engine, name, image, env=None):
            return {'Architecture': images.ARCHITECTURES[machine], 'Id': 'sha256:capture-id'}[name]
        env = {'OXPINYIN_CAPTURE_RUNTIME': runtime, 'OXPINYIN_CAPTURE_BUILD_FLAGS': '--network host', 'PATH': '',
               'OXPINYIN_PROVISION_CACHE': str(self.provision)}
        user = ['--user', f'{uid[0]}:{uid[1]}'] if uid else []
        with (mock.patch.object(images.platform, 'machine', return_value=machine),
              mock.patch.object(images.shutil, 'which', return_value=runtime),
              mock.patch.object(images, 'image_tag', return_value='TAG'),
              mock.patch.object(images, 'capture_recipe', return_value='RECIPE'),
              mock.patch.object(images, 'quiet', return_value=0 if present else 1) as quiet,
              mock.patch.object(images, 'run', fake_run),
              mock.patch.object(images, 'image_field', field),
              mock.patch.object(images, 'capture', return_value=mock.Mock(stdout=b'downloading\n/cache/data\n')),
              mock.patch.object(images, 'run_user', return_value=user),
              mock.patch.object(images.Path, 'mkdir')):
            status = images.capture_pin(list(argv), root=ROOT, env=env)
        return status, calls, [call.args[0] for call in quiet.call_args_list]

    def expected_run(self, runtime, *, platform=('--platform', 'linux/amd64'), user=('--user', '1000:1001')):
        python = 'python' + re.match(r'[0-9]+\.[0-9]+', (ROOT / '.python-version').read_text()).group()
        return [runtime, 'run', '--rm', *platform, '--network', 'none', *user,
                '-e', 'HOME=/tmp', '-e', 'TMPDIR=/tmp', '-e', 'PYTHONPYCACHEPREFIX=/tmp/pyc',
                '-e', 'OXPINYIN_CAPTURE_IMAGE=sha256:capture-id',
                '-e', 'OXPINYIN_TKRZW_LIBRARY=/usr/lib/x86_64-linux-gnu/libtkrzw.so.1',
                '-v', f'{ROOT}:/work:ro', '-v', f'{ROOT / "target/pin-observations"}:/work/target/pin-observations:rw',
                '-v', '/cache/data:/data:ro', '-v', f'{self.provision}:/provision:ro',
                '-v', f'{self.provision / "model20"}:/model20:ro', '-w', '/work', 'sha256:capture-id', python,
                '-X', 'lazy_imports=normal', 'tools/oracle/pin_capture.py', 'capture', '1', '--prefix',
                '/opt/reference/tkrzw', '--data', '/data', '--provision-cache', '/provision',
                '--model-dir', '/model20/extracted', '--staging', '/work/target/pin-observations']

    def test_docker_runs_as_the_invoking_user_with_its_platform(self):
        status, calls, inspected = self.launch('docker')
        self.assertEqual(status, 0)
        self.assertEqual(inspected, [['docker', 'image', 'inspect', 'ghcr.io/shenghaoc/oxpinyin-oracle:TAG'],
                                     ['docker', 'image', 'inspect', 'oxpinyin-capture:TAG-RECIPE']])
        self.assertEqual(calls, [self.expected_run('docker')])

    def test_podman_runs_without_a_user(self):
        _, calls, _ = self.launch('/usr/bin/podman')
        self.assertEqual(calls, [self.expected_run('/usr/bin/podman', user=())])

    def test_wslc_drops_platform_and_mounts_native_paths(self):
        for runtime in ('wslc', 'C:/Program Files/WSL/wslc.exe'):
            with self.subTest(runtime=runtime):
                _, calls, _ = self.launch(runtime, uid=None)
                self.assertEqual(calls, [self.expected_run(runtime, platform=(), user=())])
                self.assertFalse(any('cygpath' in part or part.startswith('/c/') for part in calls[0]))

    def test_absent_images_are_pulled_and_built_per_runtime(self):
        builds = {
            'docker': ['docker', 'build', '--platform', 'linux/amd64', '--pull=false'],
            'podman': ['podman', 'build', '--platform', 'linux/amd64', '--pull=never'],
            'wslc': ['wslc', 'build'],
        }
        pulls = {'docker': ['docker', 'pull', '--platform', 'linux/amd64'], 'podman': ['podman', 'pull', '--platform', 'linux/amd64'],
                 'wslc': ['wslc', 'pull']}
        for runtime, build in builds.items():
            with self.subTest(runtime=runtime):
                _, calls, _ = self.launch(runtime, present=False)
                self.assertEqual(calls[0], [*pulls[runtime], 'ghcr.io/shenghaoc/oxpinyin-oracle:TAG'])
                self.assertEqual(calls[1], [*build, '--network', 'host', '-f', 'tools/oracle/Dockerfile.capture',
                                            '--build-arg', 'REFERENCE_IMAGE=ghcr.io/shenghaoc/oxpinyin-oracle:TAG',
                                            '-t', 'oxpinyin-capture:TAG-RECIPE', '.'])

    def test_arm64_uses_the_suffixed_reference_and_its_multiarch_library(self):
        _, calls, inspected = self.launch('docker', machine='aarch64')
        self.assertEqual(inspected[0][-1], 'ghcr.io/shenghaoc/oxpinyin-oracle:TAG-arm64')
        self.assertIn('OXPINYIN_TKRZW_LIBRARY=/usr/lib/aarch64-linux-gnu/libtkrzw.so.1', calls[0])
        self.assertEqual(calls[0][3:5], ['--platform', 'linux/arm64'])

    def test_a_missing_published_image_names_build_image(self):
        def fail_pull(command, **kwargs):
            return 1
        with (mock.patch.object(images.platform, 'machine', return_value='x86_64'),
              mock.patch.object(images.shutil, 'which', return_value='docker'),
              mock.patch.object(images, 'image_tag', return_value='TAG'),
              mock.patch.object(images, 'quiet', return_value=1), mock.patch.object(images, 'run', fail_pull),
              contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(images.Finished)):
            images.capture_pin(['1'], root=ROOT, env={'PATH': '', 'OXPINYIN_PROVISION_CACHE': str(self.provision)})
        self.assertIn('is unavailable; build it with: uv run --no-project --managed-python python -X lazy_imports=normal '
                      '-m tools.oracle.image build-image', error.getvalue())

    def test_a_missing_provision_or_model_cache_is_refused_before_any_runtime_call(self):
        model = Path(self.provision.parent) / 'model-elsewhere'
        for env, message in (({}, 'set OXPINYIN_PROVISION_CACHE to a `tools.dev provision oracle --cache-dir` directory'),
                             ({'OXPINYIN_PROVISION_CACHE': str(self.provision.parent / 'absent')}, 'set OXPINYIN_PROVISION_CACHE'),
                             ({'OXPINYIN_PROVISION_CACHE': str(self.provision), 'PINYIN_MODEL_CACHE': str(model)},
                              f'{model / "extracted"} is not a model20 extraction; set PINYIN_MODEL_CACHE')):
            with (self.subTest(env=env), mock.patch.object(images, 'run') as run, mock.patch.object(images, 'quiet') as quiet,
                  contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(images.Finished) as raised):
                images.capture_pin(['1'], root=ROOT, env=dict(env, PATH=''))
            self.assertEqual(raised.exception.status, 2)
            self.assertIn(message, error.getvalue())
            run.assert_not_called()
            quiet.assert_not_called()

    def test_the_model_cache_defaults_to_the_provision_caches_model20(self):
        _, calls, _ = self.launch('docker')
        self.assertIn(f'{self.provision / "model20"}:/model20:ro', calls[0])

    def test_foreign_architecture_and_bad_run_names_are_refused_before_any_runtime_call(self):
        for argv, machine in ((['1'], 'riscv64'), (['../1'], 'x86_64'), (['a b'], 'x86_64'), ([], 'x86_64')):
            with self.subTest(argv=argv, machine=machine), \
                    mock.patch.object(images.platform, 'machine', return_value=machine), \
                    mock.patch.object(images.shutil, 'which', return_value='docker'), \
                    mock.patch.object(images, 'run') as run, mock.patch.object(images, 'quiet') as quiet, \
                    contextlib.redirect_stderr(io.StringIO()), self.assertRaises(images.Finished) as raised:
                images.capture_pin(argv, root=ROOT, env={'PATH': ''})
            self.assertEqual(raised.exception.status, 2)
            run.assert_not_called()
            quiet.assert_not_called()

    def test_the_run_user_is_the_invoking_uid_and_none_without_one(self):
        posix = types.SimpleNamespace(getuid=lambda: 1000, getgid=lambda: 1001)
        with mock.patch.object(images, 'os', posix):
            self.assertEqual(images.run_user(), ['--user', '1000:1001'])
        with mock.patch.object(images, 'os', types.SimpleNamespace()):
            self.assertEqual(images.run_user(), [])

    def test_the_capture_recipe_hashes_sha256sum_text_lines(self):
        lines = ''.join(f'{images.digest_file(ROOT / name)}  {name}\n' for name in images.CAPTURE_RECIPE)
        self.assertEqual(images.capture_recipe(ROOT), hashlib.sha256(lines.encode()).hexdigest()[:12])

if __name__ == '__main__':
    unittest.main()
