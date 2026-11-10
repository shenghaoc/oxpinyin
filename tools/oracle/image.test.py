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


def as_macos(test):
    """`container` names Apple's CLI only on macOS (on Windows it is wslc's alias): run TEST there."""
    patcher = mock.patch.object(images, 'host_system', return_value='Darwin')
    patcher.start()
    test.addCleanup(patcher.stop)


class PerformanceImageTests(unittest.TestCase):
    def setUp(self):
        as_macos(self)

    def test_native_platform_is_explicit_for_build_and_run(self):
        with mock.patch.dict(os.environ, {}, clear=True), mock.patch.object(images.platform, 'machine', return_value='arm64'), \
             mock.patch.object(images, 'run', return_value=0) as run, \
             mock.patch.object(images, 'text', return_value='/checkout/.git'):
            self.assertEqual(images.performance(['--engine', 'container', '--', 'true']), 0)
        build, launch = [call.args[0] for call in run.call_args_list]
        self.assertEqual(build[build.index('--platform') + 1], 'linux/arm64')
        self.assertEqual(build[build.index('--memory') + 1], '6G')
        self.assertIn('type=bind,source=/checkout/.git,target=/checkout/.git,readonly', launch)
        self.assertIn(f'type=bind,source={ROOT.resolve()},target=/repo,readonly', launch)
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

    def test_reference_tag_follows_every_recipe_input(self):
        result = self.invoke(['image-tag'], [])
        self.assertEqual(result.stdout.decode().strip(), images.image_tag())
        before = images.image_tag()
        copied = self.directory / 'tree'
        for path in ('tools/oracle/Dockerfile.oracle', 'tools/oracle/build.py', 'tools/oracle/version-manifest.json',
                     'tools/oracle/versions.py', 'tools/oracle/oracle-pin.txt'):
            dest = copied / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / path, dest)
        self.assertEqual(images.image_tag(copied), before)
        builder = copied / 'tools/oracle/build.py'
        original_builder = builder.read_bytes()
        builder.write_bytes(b'changed builder')
        self.assertNotEqual(images.image_tag(copied), before)
        builder.write_bytes(original_builder)
        (copied / 'tools/oracle/version-manifest.json').write_bytes(b'changed input')
        self.assertNotEqual(images.image_tag(copied), before)

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
        log = self.directory / 'calls'
        body = f"""import json,pathlib,sys
with pathlib.Path({str(log)!r}).open('a') as f: f.write(json.dumps(sys.argv[1:])+"\\n")
if sys.argv[1]=='create': print('owned-id')
if sys.argv[1]=='cp': sys.exit(19)
"""
        if sys.platform == 'win32':
            # Windows runs no shebang script: the fake engine is a .cmd running the same Python.
            (self.directory / 'docker.py').write_text(body)
            engine = self.directory / 'docker.cmd'
            engine.write_text(f'@"{sys.executable}" "%~dp0docker.py" %*\r\n@exit /b %ERRORLEVEL%\r\n')
        else:
            engine = self.directory / 'docker'
            engine.write_text(f'#!{sys.executable}\n' + body)
            engine.chmod(0o755)
        args = ['--engine', str(engine), 'image']
        self.assertEqual(self.invoke(['verify-image'], args).returncode, 19)
        rows = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertEqual([row[0] for row in rows], ['create', 'cp', 'rm'])
        self.assertEqual(rows[0], ['create', '--pull=never', '--platform', images.host_platform(), 'image', '/none'])
        self.assertEqual(rows[1][:2], ['cp', 'owned-id:/opt/pinyin-oracle'])
        self.assertEqual([row for row in rows if row[0] == 'rm'], [['rm', '-f', 'owned-id']])

    def test_runtime_preference_and_explicit_override_are_preserved(self):
        program = 'from tools.oracle.image import select_runtime; print("selected:" + select_runtime())'
        if sys.platform == 'win32':
            # Windows names its host itself (no uname) and orders wslc, podman, docker. Each fake
            # runtime is a copy of this interpreter, which answers --version. PATH holds only the
            # fakes and the interpreter's own directory, so no installed runtime is selected.
            for kind in ('wslc', 'podman', 'docker'):
                shutil.copyfile(sys.executable, self.directory / (kind + '.exe'))
            env = dict(self.env, PATH=os.pathsep.join([str(self.directory), sys.base_prefix]),
                       PYTHONHOME=sys.base_prefix)
            for absent, override, expected in ((None, '', 'wslc'), ('wslc', '', 'podman'), (None, 'docker', 'docker')):
                with self.subTest(absent=absent, override=override):
                    if absent:
                        (self.directory / (absent + '.exe')).unlink()
                    result = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', program], cwd=ROOT,
                                            env=dict(env, OXPINYIN_CONTAINER=override), capture_output=True, text=True)
                    if absent:
                        shutil.copyfile(sys.executable, self.directory / (absent + '.exe'))
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn('selected:' + expected, result.stdout)
            return
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
            result = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', program],
                                    cwd=ROOT, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('selected:' + expected, result.stdout)


class WslcAdapterTests(unittest.TestCase):
    """The wslc translation of each kept command; no container runs."""
    def setUp(self):
        as_macos(self)
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

    def verify_through_export(self, removal_status=0, engine='wslc'):
        """verify-image on ENGINE over a synthetic export; answer status, calls, stderr and the removal mock."""
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
            status = images.verify_image(['--engine', engine, '--platform', platform, 'image'])
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
              mock.patch.object(images, 'select_runtime', return_value=runtime) as select,
              contextlib.redirect_stdout(io.StringIO()) as output):
            self.assertEqual(images.build_image([], env=env), 0)
        select.assert_called_once_with(override=runtime, env=env)
        return calls, field.call_args, verify.call_args, output.getvalue()

    def test_the_capture_runtime_variable_overrides_oxpinyin_container(self):
        def version(command, **kwargs):
            return mock.Mock(stdout=b'synthetic 1.0\n', status=0)
        with (mock.patch.object(images.shutil, 'which', side_effect=lambda name, path=None: name),
              mock.patch.object(images, 'capture', version)):
            selected = images.select_runtime(override='wslc', env={'OXPINYIN_CONTAINER': 'docker', 'PATH': ''},
                                             host='Linux', output=lambda line: None)
        self.assertEqual(selected, 'wslc')

    def test_build_image_keeps_the_docker_command_and_translates_wslc(self):
        calls, field, verify, output = self.build_calls('docker', 'x86_64')
        image = 'ghcr.io/shenghaoc/oxpinyin-oracle:TAG'
        self.assertEqual(calls, [['docker', 'build', '--platform', 'linux/amd64', '--build-arg', 'ORACLE_PLATFORM=linux/amd64',
                                  '--network', 'host', '-f', 'tools/oracle/Dockerfile.oracle', '-t', image, '.']])
        self.assertEqual(field.args, ('docker', 'Architecture', image))
        self.assertEqual(verify.args[0], ['--engine', 'docker', '--platform', 'linux/amd64', image])
        self.assertEqual(output, image + '\n')
        calls, field, verify, output = self.build_calls('wslc', 'aarch64')
        # wslc's build has no --network: the site flag is dropped.
        self.assertEqual(calls, [['wslc', 'build', '--build-arg', 'ORACLE_PLATFORM=linux/arm64',
                                  '-f', 'tools/oracle/Dockerfile.oracle', '-t', image + '-arm64', '.']])
        self.assertEqual(verify.args[0], ['--engine', 'wslc', '--platform', 'linux/arm64', image + '-arm64'])

    def test_build_image_refuses_another_architecture_before_building(self):
        with (mock.patch.object(images.platform, 'machine', return_value='riscv64'),
              mock.patch.object(images, 'select_runtime') as select,
              mock.patch.object(images, 'run') as run, contextlib.redirect_stderr(io.StringIO()),
              self.assertRaises(images.Finished) as raised):
            images.build_image([], env={})
        self.assertEqual(raised.exception.status, 2)
        run.assert_not_called()
        select.assert_not_called()

    def test_wslc_is_selected_first_on_windows_and_reports_its_version_line(self):
        def fake(command, **kwargs):
            return mock.Mock(stdout=b'wslc 3.0.3.0\nCopyright (c) Microsoft Corporation.\n', status=0)
        printed = []
        with mock.patch.object(images.shutil, 'which', side_effect=lambda name, path=None: name), \
                mock.patch.object(images, 'capture', fake):
            selected = images.select_runtime(env={'PATH': ''}, host='MINGW64_NT-10.0', output=printed.append)
        self.assertEqual(selected, 'wslc')
        self.assertEqual(printed, ['Runtime: wslc; wslc 3.0.3.0'])

class RuntimeAdapterTests(unittest.TestCase):
    """runtime_command for every runtime select_runtime returns; no container runs."""
    RUN = ['run', '--rm', '--platform', 'linux/amd64', '--network', 'host', '-v', '/r:/opt/r:ro,z',
           '-v', '/o:/opt/o:z', 'image', 'python3', '/opt/r/tools/oracle/build.py']

    def setUp(self):
        as_macos(self)

    def test_docker_and_podman_take_every_command_as_written(self):
        for engine in ('docker', '/usr/bin/podman'):
            for command in (self.RUN, ['info'], ['pull', '--quiet', '--platform', 'linux/amd64', 'i'], ['tag', 'a', 'b'],
                            ['create', '--pull=never', 'i', '/none'], ['rm', '-f', 'id'], ['container', 'rm', '-f', 'id']):
                with self.subTest(engine=engine, command=command):
                    self.assertEqual(images.runtime_command([engine, *command]), [engine, *command])

    def test_wslc_drops_platform_relabelling_and_host_networking(self):
        # wslc run: "host mode networking is not supported"; --network none is kept.
        self.assertEqual(images.runtime_command(['wslc', *self.RUN]),
                         ['wslc', 'run', '--rm', '-v', '/r:/opt/r:ro', '-v', '/o:/opt/o', 'image', 'python3', '/opt/r/tools/oracle/build.py'])
        self.assertEqual(images.runtime_command(['wslc', 'run', '--network', 'none', 'i']), ['wslc', 'run', '--network', 'none', 'i'])
        self.assertEqual(images.runtime_command(['wslc', 'build', '--platform', 'linux/amd64', '--network', 'host',
                                                 '--target', 'deps', '.']), ['wslc', 'build', '--target', 'deps', '.'])
        self.assertEqual(images.runtime_command(['wslc', 'container', 'rm', '-f', 'id']),
                         ['wslc', 'container', 'remove', '-f', 'id'])
        self.assertEqual(images.runtime_command(['wslc', 'info']), ['wslc', 'info'])
        self.assertEqual(images.runtime_command(['wslc', 'tag', 'a', 'b']), ['wslc', 'tag', 'a', 'b'])

    def test_apple_container_groups_renames_and_mounts_read_only_binds(self):
        self.assertEqual(images.runtime_command(['container', *self.RUN]),
                         ['container', 'run', '--rm', '--platform', 'linux/amd64',
                          '--mount', 'type=bind,source=/r,target=/opt/r,readonly', '-v', '/o:/opt/o',
                          'image', 'python3', '/opt/r/tools/oracle/build.py'])
        for command, expected in ((['info'], ['system', 'status']),
                                  (['pull', '--quiet', '--platform', 'linux/arm64', 'i'],
                                   ['image', 'pull', '--platform', 'linux/arm64', 'i']),
                                  (['tag', 'a', 'b'], ['image', 'tag', 'a', 'b']),
                                  (['rm', '-f', 'id'], ['delete', '-f', 'id']),
                                  (['container', 'rm', '-f', 'id'], ['delete', '-f', 'id']),
                                  (['container', 'inspect', 'id'], ['inspect', 'id']),
                                  (['create', '--pull=never', '--platform', 'linux/amd64', 'i', '/none'],
                                   ['create', '--platform', 'linux/amd64', 'i', '/none']),
                                  (['build', '--platform', 'linux/amd64', '--network', 'host', '--target', 'deps', '.'],
                                   ['build', '--platform', 'linux/amd64', '--target', 'deps', '.']),
                                  (['image', 'inspect', 'i'], ['image', 'inspect', 'i'])):
            with self.subTest(command=command):
                self.assertEqual(images.runtime_command(['/usr/local/bin/container', *command]),
                                 ['/usr/local/bin/container', *expected])

    def test_container_names_apple_only_on_macos(self):
        for host, apple in (('Darwin', True), ('Windows', False), ('Linux', False)):
            with self.subTest(host=host), mock.patch.object(images, 'host_system', return_value=host):
                self.assertIs(images.is_apple('container'), apple)
                self.assertIs(images.is_apple('/usr/local/bin/container'), apple)
        with mock.patch.object(images, 'host_system', return_value='Windows'):
            self.assertEqual(images.runtime_command(['container.exe', 'create', '--platform', 'linux/amd64', 'i', '/none']),
                             ['container.exe', 'create', 'i', '/none'])

    def test_host_networking_is_dropped_with_a_notice(self):
        for command in (['wslc', 'run', '--network', 'host', 'i'], ['wslc', 'build', '--network=host', '.'],
                        ['container', 'run', '--network', 'host', 'i'], ['container', 'build', '--network', 'host', '.']):
            with self.subTest(command=command), contextlib.redirect_stderr(io.StringIO()) as error:
                translated = images.runtime_command(command)
            self.assertFalse(any('--network' in str(part) for part in translated), translated)
            self.assertEqual(error.getvalue(), f"{command[0]}: no --network host; the runtime's default network is used\n")

    def test_a_network_the_runtime_cannot_honour_is_refused(self):
        for command in (['wslc', 'build', '--network', 'none', '.'], ['wslc', 'build', '--network=lab', '.'],
                        ['container', 'build', '--network', 'none', '.'], ['container', 'build', '--network', 'lab', '.']):
            with (self.subTest(command=command), contextlib.redirect_stderr(io.StringIO()) as error,
                  self.assertRaises(images.Finished) as raised):
                images.runtime_command(command)
            self.assertEqual(raised.exception.status, 2)
            value = command[3] if len(command) > 4 else command[2].split('=', 1)[1]
            self.assertIn(f'{command[0]} build has no --network; it cannot honour --network {value}', error.getvalue())

    def test_the_tools_own_run_network_none_is_kept_quietly(self):
        for engine in ('wslc', 'container'):
            for value in ('none', 'lab'):
                with self.subTest(engine=engine, value=value), contextlib.redirect_stderr(io.StringIO()) as error:
                    translated = images.runtime_command([engine, 'run', '--network', value, 'i'])
                self.assertEqual(translated[translated.index('--network') + 1], value)
                self.assertEqual(error.getvalue(), '')

    def test_apple_architecture_is_the_requested_platforms_variant(self):
        # The index lists arm64 before amd64: the requested platform picks, not the first linux entry.
        record = [{'configuration': {'descriptor': {'digest': 'sha256:cfg'}}, 'variants': [
            {'platform': {'os': 'linux', 'architecture': 'arm64'}},
            {'platform': {'os': 'linux', 'architecture': 'amd64'}},
            {'platform': {'os': 'unknown', 'architecture': 'unknown'}}]}]
        with mock.patch.object(images, 'capture', return_value=mock.Mock(stdout=json.dumps(record).encode())):
            self.assertEqual(images.image_field('container', 'Architecture', 'i', platform='linux/amd64'), 'amd64')
            self.assertEqual(images.image_field('container', 'Architecture', 'i', platform='linux/arm64'), 'arm64')
            with mock.patch.object(images, 'host_platform', return_value='linux/amd64'):
                self.assertEqual(images.image_field('container', 'Architecture', 'i'), 'amd64')
            self.assertEqual(images.image_field('container', 'Architecture', 'i', platform='linux/s390x'), 'arm64,amd64')

    def test_windows_mount_sources_keep_their_drive_colon(self):
        self.assertEqual(images.runtime_command(['container', 'run', '-v', 'C:\\r:/repo:ro', 'i']),
                         ['container', 'run', '--mount', 'type=bind,source=C:\\r,target=/repo,readonly', 'i'])
        self.assertEqual(images.runtime_command(['wslc', 'run', '-v', 'C:\\r:/repo:ro,z', 'i']),
                         ['wslc', 'run', '-v', 'C:\\r:/repo:ro', 'i'])

    def test_image_field_reads_apple_index_records(self):
        record = [{'configuration': {'descriptor': {'digest': 'sha256:idx'}}, 'id': 'idx', 'variants': [
            {'platform': {'os': 'unknown', 'architecture': 'unknown'}},
            {'platform': {'os': 'linux', 'architecture': 'arm64'}}]}]
        with mock.patch.object(images, 'capture', return_value=mock.Mock(stdout=json.dumps(record).encode())) as capture:
            self.assertEqual(images.image_field('container', 'Architecture', 'i'), 'arm64')
            self.assertEqual(images.image_field('container', 'Id', 'i'), 'sha256:idx')
        self.assertEqual(capture.call_args.args[0], ['container', 'image', 'inspect', 'i'])

    def test_container_owner_per_runtime(self):
        cid = 'c' * 64
        answers = {'docker': f'{cid} tok\n', 'wslc': json.dumps([{'Id': cid, 'Config': {'Labels': {'k': 'tok'}}}]),
                   'container': json.dumps([{'configuration': {'id': cid, 'labels': {'k': 'tok'}}}])}
        commands = {'docker': ['docker', 'container', 'inspect', '--format', '{{.Id}} {{index .Config.Labels "k"}}', cid],
                    'wslc': ['wslc', 'container', 'inspect', cid], 'container': ['container', 'inspect', cid]}
        for engine, answer in answers.items():
            with self.subTest(engine=engine):
                calls = []

                def run(argv, answer=answer):
                    calls.append(argv)
                    return subprocess.CompletedProcess(argv, 0, answer, '')
                self.assertEqual(images.container_owner((engine,), cid, 'k', run=run), f'{cid} tok')
                self.assertEqual(calls, [commands[engine]])
        def failed(argv):
            return subprocess.CompletedProcess(argv, 1, '', '')

        def garbled(argv):
            return subprocess.CompletedProcess(argv, 0, 'not json', '')
        for engine in ('docker', 'wslc', 'container'):
            self.assertIsNone(images.container_owner((engine,), cid, 'k', run=failed))
        self.assertIsNone(images.container_owner(('wslc',), cid, 'k', run=garbled))

    def test_container_ids_per_runtime(self):
        mine, other = 'c' * 64, 'd' * 64
        listing = [{'configuration': {'id': mine, 'labels': {'k': 'tok'}}},
                   {'configuration': {'id': other, 'labels': {'k': 'someone-else'}}},
                   {'configuration': {'id': 'e' * 64}}]
        answers = {'docker': f'{mine}\n', 'wslc': f'{mine}\r\n', 'container': json.dumps(listing)}
        commands = {'docker': ['docker', 'container', 'ls', '-a', '-q', '--no-trunc', '--filter', 'label=k=tok'],
                    'wslc': ['wslc', 'container', 'ls', '-a', '-q', '--no-trunc', '--filter', 'label=k=tok'],
                    'container': ['container', 'ls', '-a', '--format', 'json']}
        for engine, answer in answers.items():
            with self.subTest(engine=engine):
                calls = []

                def run(argv, answer=answer):
                    calls.append(argv)
                    return subprocess.CompletedProcess(argv, 0, answer, '')
                self.assertEqual(images.container_ids((engine,), 'k', 'tok', run=run), [mine])
                self.assertEqual(calls, [commands[engine]])

        def failed(argv):
            return subprocess.CompletedProcess(argv, 1, '', '')

        def garbled(argv):
            return subprocess.CompletedProcess(argv, 0, 'not json', '')
        for engine in ('docker', 'wslc', 'container'):
            self.assertEqual(images.container_ids((engine,), 'k', 'tok', run=failed), [])
        self.assertEqual(images.container_ids(('container',), 'k', 'tok', run=garbled), [])


class ProvisionOracleTests(unittest.TestCase):
    """just provision-oracle: the performance image build and the provision oracle run, per runtime."""
    def setUp(self):
        as_macos(self)
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.cache = Path(directory.name) / 'cache'

    def calls(self, engine, argv=(), env=None, present=False):
        calls = []

        def fake_run(command, **kwargs):
            calls.append([str(part) for part in command])
            return 0
        environment = dict(env or {}, OXPINYIN_PROVISION_CACHE=str(self.cache))
        with (mock.patch.object(images.platform, 'machine', return_value='x86_64'),
              mock.patch.object(images, 'select_runtime', return_value=engine),
              mock.patch.object(images, 'image_tag', return_value='TAG'),
              mock.patch.object(images, 'quiet', return_value=0 if present else 1),
              mock.patch.object(images, 'run', fake_run), contextlib.redirect_stderr(io.StringIO())):
            self.assertEqual(images.provision_oracle(list(argv), env=environment), 0)
        return calls

    def test_each_runtime_builds_the_performance_target_and_runs_provision_oracle(self):
        for engine in ('docker', 'podman', 'wslc', 'container'):
            with self.subTest(engine=engine):
                build, launch = self.calls(engine, ['--cells', 'tkrzw'])
                self.assertEqual(build[build.index('--target') + 1], 'performance')
                self.assertIn('ORACLE_PLATFORM=linux/amd64', build)
                self.assertEqual('--platform' in build, engine != 'wslc')
                self.assertEqual('--platform' in launch, engine != 'wslc')
                self.assertEqual('--memory' in build, engine == 'container')
                if engine == 'container':
                    self.assertIn(f'type=bind,source={ROOT.resolve()},target=/repo,readonly', launch)
                else:
                    self.assertIn(f'{ROOT.resolve()}:/repo:ro', launch)
                self.assertIn(f'{self.cache.resolve()}:/provision', launch)
                self.assertEqual(launch[launch.index('provision'):],
                                 ['provision', 'oracle', '--cache-dir', '/provision', '--cells', 'tkrzw'])
                self.assertTrue(launch[launch.index('-w') + 2].startswith('oxpinyin-performance:TAG-'))

    def test_rootful_docker_runs_as_the_host_user_with_a_writable_uv(self):
        with mock.patch.object(images, 'run_user', return_value=['--user', '1000:1001']):
            commands = {engine: images.provision_oracle_command(engine, ROOT, self.cache, 'tag', 'linux/amd64', [])
                        for engine in ('docker', 'podman', 'container', 'wslc')}
        docker = commands['docker']
        self.assertEqual(docker[docker.index('--user') + 1], '1000:1001')
        self.assertIn('HOME=/tmp', docker)
        self.assertIn('UV_CACHE_DIR=/tmp/uv-cache', docker)
        self.assertLess(docker.index('UV_CACHE_DIR=/tmp/uv-cache'), docker.index('tag'))
        for engine in ('podman', 'container'):
            self.assertNotIn('--user', commands[engine], engine)
            self.assertNotIn('UV_CACHE_DIR=/tmp/uv-cache', commands[engine], engine)
        with mock.patch.object(images, 'run_user', return_value=[]):
            self.assertNotIn('--user', images.provision_oracle_command('wslc', ROOT, self.cache, 'tag', 'linux/amd64', []))

    def test_other_options_ci_and_foreign_architectures_are_refused_before_any_runtime_call(self):
        for argv, env, machine in ((['--cache-dir', '/x'], {}, 'x86_64'), (['--cells'], {}, 'x86_64'),
                                   ([], {'GITHUB_ACTIONS': 'true'}, 'x86_64'), ([], {}, 'riscv64')):
            with (self.subTest(argv=argv, env=env, machine=machine),
                  mock.patch.object(images.platform, 'machine', return_value=machine),
                  mock.patch.object(images, 'select_runtime') as select, mock.patch.object(images, 'run') as run,
                  contextlib.redirect_stderr(io.StringIO()), self.assertRaises(images.Finished) as raised):
                images.provision_oracle(argv, env=env)
            self.assertEqual(raised.exception.status, 2)
            select.assert_not_called()
            run.assert_not_called()


class AppleSavedImageTests(unittest.TestCase):
    """verify-image on Apple's container: `image save` and the OCI layout, with a stub engine."""
    GZIP = 'application/vnd.oci.image.layer.v1.tar+gzip'

    def setUp(self):
        as_macos(self)
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)

    @staticmethod
    def layer(files, extra=()):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode='w:gz') as tar:
            for name, data in files.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
            for name in extra:
                info = tarfile.TarInfo(name)
                tar.addfile(info, io.BytesIO(b''))
        return buffer.getvalue()

    def archive(self, path, *, platform=('linux', 'amd64'), media=GZIP, corrupt=False, whiteout=False):
        """Write a saved-image archive: index.json -> nested index -> manifest -> two layers."""
        blobs = {}

        def add(data):
            digest = hashlib.sha256(data).hexdigest()
            blobs[digest] = data
            return digest
        first = {f'opt/pinyin-oracle/{dbm}/lib/{library}.so': f'{dbm} {library} first'.encode()
                 for dbm in images.cell.CELLS for library in ('libpinyin', 'libzhuyin')}
        first['etc/passwd'] = b'outside the oracle\n'
        second = {'opt/pinyin-oracle/tkrzw/lib/libpinyin.so': b'tkrzw libpinyin second'}
        layers = [self.layer(first), self.layer(second, ['opt/pinyin-oracle/kc/lib/.wh.libzhuyin.so'] if whiteout else [])]
        descriptors = [{'mediaType': media, 'digest': 'sha256:' + add(data), 'size': len(data)} for data in layers]
        config = add(b'{}')
        manifest = json.dumps({'schemaVersion': 2, 'mediaType': 'application/vnd.oci.image.manifest.v1+json',
                               'config': {'mediaType': 'application/vnd.oci.image.config.v1+json',
                                          'digest': 'sha256:' + config, 'size': 2},
                               'layers': descriptors}).encode()
        manifest_digest = add(manifest)
        nested = json.dumps({'schemaVersion': 2, 'mediaType': 'application/vnd.oci.image.index.v1+json', 'manifests': [
            {'mediaType': 'application/vnd.oci.image.manifest.v1+json', 'digest': 'sha256:' + manifest_digest,
             'size': len(manifest), 'platform': {'os': platform[0], 'architecture': platform[1]}},
            {'mediaType': 'application/vnd.oci.image.manifest.v1+json', 'digest': 'sha256:' + manifest_digest,
             'size': len(manifest), 'platform': {'os': 'unknown', 'architecture': 'unknown'}}]}).encode()
        nested_digest = add(nested)
        top = json.dumps({'schemaVersion': 2, 'manifests': [
            {'mediaType': 'application/vnd.oci.image.index.v1+json', 'digest': 'sha256:' + nested_digest,
             'size': len(nested)}]}).encode()
        if corrupt:
            digest = descriptors[1]['digest'].split(':')[1]
            blobs[digest] = blobs[digest][:-1] + bytes([blobs[digest][-1] ^ 1])
        with tarfile.open(path, 'w') as tar:
            for name, data in (('oci-layout', b'{"imageLayoutVersion": "1.0.0"}'), ('index.json', top),
                               *((f'blobs/sha256/{digest}', data) for digest, data in blobs.items())):
                info = tarfile.TarInfo(name)
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))

    def stub(self, calls, **archive):
        """The stub engine: `image save -o PATH` writes the synthetic archive; nothing else may run."""
        def run(command, **kwargs):
            command = [str(part) for part in command]
            calls.append(command)
            if command[1:3] != ['image', 'save']:
                raise AssertionError(f'unexpected runtime command: {command}')
            self.archive(Path(command[command.index('-o') + 1]), **archive)
            return 0
        return run

    def saved(self, **archive):
        calls = []
        with mock.patch.object(images, 'run', self.stub(calls, **archive)):
            oracle = images.saved_oracle('container', 'image', 'linux/amd64', self.directory)
        return oracle, calls

    def test_layers_apply_in_order_and_only_the_oracle_is_extracted(self):
        oracle, calls = self.saved()
        self.assertEqual(calls, [['container', 'image', 'save', '-o', str(self.directory / 'image.tar'),
                                  '--platform', 'linux/amd64', 'image']])
        self.assertEqual((oracle / 'tkrzw/lib/libpinyin.so').read_bytes(), b'tkrzw libpinyin second')
        self.assertEqual((oracle / 'bdb/lib/libpinyin.so').read_bytes(), b'bdb libpinyin first')
        self.assertEqual(oracle, self.directory / 'saved/opt/pinyin-oracle')
        self.assertFalse((self.directory / 'saved/etc').exists())
        self.assertFalse((self.directory / 'image.tar').exists())

    def test_verify_image_creates_no_container(self):
        native = images.host_platform()
        calls = []
        with (mock.patch.object(images, 'run', self.stub(calls, platform=tuple(native.split('/')))),
              mock.patch.object(images, 'capture', side_effect=AssertionError('no create on Apple')),
              mock.patch.object(images.subprocess, 'run', side_effect=AssertionError('no removal on Apple')),
              mock.patch.object(images, 'prefix_ok', return_value=True),
              contextlib.redirect_stdout(io.StringIO()) as output):
            self.assertEqual(images.verify_image(['--engine', 'container', '--platform', native, 'image']), 0)
        self.assertEqual([call[1:3] for call in calls], [['image', 'save']])
        self.assertFalse(any(word in call for call in calls for word in ('create', 'cp', 'export')))
        self.assertIn(f'tkrzw libpinyin {hashlib.sha256(b"tkrzw libpinyin second").hexdigest()}', output.getvalue())

    def refused(self, message, **archive):
        with contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(images.Finished):
            self.saved(**archive)
        self.assertIn(message, error.getvalue())

    def test_a_blob_that_does_not_match_its_digest_fails(self):
        self.refused('does not match its digest', corrupt=True)

    def test_no_entry_for_the_platform_fails(self):
        self.refused('no linux/amd64 entry in the saved image index (linux/arm64, unknown/unknown)',
                     platform=('linux', 'arm64'))

    def test_an_unknown_layer_media_type_fails(self):
        self.refused("has media type 'application/vnd.oci.image.layer.v1.tar+zstd'",
                     media='application/vnd.oci.image.layer.v1.tar+zstd')

    def test_a_whiteout_under_the_oracle_fails(self):
        self.refused('whites out opt/pinyin-oracle/kc/lib/.wh.libzhuyin.so', whiteout=True)


class CaptureLauncherTests(unittest.TestCase):
    """The commands `just capture-pin` builds for each runtime; no container runs."""
    def setUp(self):
        as_macos(self)
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.provision = Path(directory.name).resolve() / 'provision'
        (self.provision / 'model20/extracted').mkdir(parents=True)
        self.env = {'PATH': '', 'OXPINYIN_PROVISION_CACHE': str(self.provision)}

    def launch(self, runtime, *, present=True, machine='x86_64', uid=(1000, 1001), argv=('1',)):
        calls = []

        def fake_run(command, **kwargs):
            calls.append([str(part) for part in command])
            return 0

        def field(engine, name, image, env=None, platform=None):
            return {'Architecture': images.ARCHITECTURES[machine], 'Id': 'sha256:capture-id'}[name]
        env = {'OXPINYIN_CAPTURE_RUNTIME': runtime, 'OXPINYIN_CAPTURE_BUILD_FLAGS': '--network host', 'PATH': '',
               'OXPINYIN_PROVISION_CACHE': str(self.provision)}
        user = ['--user', f'{uid[0]}:{uid[1]}'] if uid else []
        with (mock.patch.object(images.platform, 'machine', return_value=machine),
              mock.patch.object(images, 'select_runtime', return_value=runtime) as select,
              mock.patch.object(images, 'image_tag', return_value='TAG'),
              mock.patch.object(images, 'capture_recipe', return_value='RECIPE'),
              mock.patch.object(images, 'quiet', return_value=0 if present else 1) as quiet,
              mock.patch.object(images, 'run', fake_run),
              mock.patch.object(images, 'image_field', field),
              mock.patch.object(images, 'capture', return_value=mock.Mock(stdout=b'downloading\n/cache/data\n')),
              mock.patch.object(images, 'run_user', return_value=user),
              mock.patch.object(images.Path, 'mkdir')):
            status = images.capture_pin(list(argv), root=ROOT, env=env)
        select.assert_called_once()
        self.assertEqual(select.call_args.kwargs['override'], runtime)
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

    def test_the_apple_capture_build_has_no_pull_argument(self):
        # Apple's build: "The option '--pull' does not take any value".
        command = images.capture_build_command('container', 'ref', 'capture:tag', selected='linux/arm64', flags=[])
        self.assertFalse(any(str(part).startswith('--pull') for part in command), command)
        self.assertEqual(command, ['container', 'build', '--cpus', '4', '--memory', '6G', '--platform', 'linux/arm64', '-f', 'tools/oracle/Dockerfile.capture',
                                   '--build-arg', 'REFERENCE_IMAGE=ref', '-t', 'capture:tag', '.'])
        for runtime, pull in (('docker', '--pull=false'), ('podman', '--pull=never')):
            self.assertIn(pull, images.capture_build_command(runtime, 'ref', 'capture:tag', selected='linux/amd64', flags=[]))

    def test_apple_runs_tag_with_digest_in_environment(self):
        status, calls, _ = self.launch('container')
        self.assertEqual(status, 0)
        self.assertIn('OXPINYIN_CAPTURE_IMAGE=sha256:capture-id', calls[0])
        self.assertIn('oxpinyin-capture:TAG-RECIPE', calls[0])
        self.assertNotIn('sha256:capture-id', calls[0])

    def test_apple_rejects_changed_capture_digest(self):
        ids = iter(['sha256:before', 'sha256:after'])
        def field(engine, name, image, env=None, platform=None):
            return 'amd64' if name == 'Architecture' else next(ids)
        with (mock.patch.object(images.platform, 'machine', return_value='x86_64'),
              mock.patch.object(images, 'select_runtime', return_value='container'),
              mock.patch.object(images, 'image_tag', return_value='TAG'),
              mock.patch.object(images, 'capture_recipe', return_value='RECIPE'),
              mock.patch.object(images, 'quiet', return_value=0),
              mock.patch.object(images, 'run', return_value=0),
              mock.patch.object(images, 'image_field', field),
              mock.patch.object(images, 'capture', return_value=mock.Mock(stdout=b'/cache/data\n')),
              mock.patch.object(images.Path, 'mkdir')):
            with self.assertRaises(images.Finished):
                images.capture_pin(['1'], root=ROOT, env=self.env)

    def test_arm64_uses_the_suffixed_reference_and_its_multiarch_library(self):
        _, calls, inspected = self.launch('docker', machine='aarch64')
        self.assertEqual(inspected[0][-1], 'ghcr.io/shenghaoc/oxpinyin-oracle:TAG-arm64')
        self.assertIn('OXPINYIN_TKRZW_LIBRARY=/usr/lib/aarch64-linux-gnu/libtkrzw.so.1', calls[0])
        self.assertEqual(calls[0][3:5], ['--platform', 'linux/arm64'])

    def test_a_missing_published_image_names_build_image(self):
        def fail_pull(command, **kwargs):
            return 1
        with (mock.patch.object(images.platform, 'machine', return_value='x86_64'),
              mock.patch.object(images, 'select_runtime', return_value='docker'),
              mock.patch.object(images, 'image_tag', return_value='TAG'),
              mock.patch.object(images, 'quiet', return_value=1), mock.patch.object(images, 'run', fail_pull),
              contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(images.Finished)):
            images.capture_pin(['1'], root=ROOT, env=self.env)
        self.assertIn('is unavailable; build it with: uv run --no-project --managed-python python -X lazy_imports=normal '
                      '-m tools.oracle.image build-image', error.getvalue())

    def test_a_missing_provision_or_model_cache_is_refused_before_any_runtime_call(self):
        model = Path(self.provision.parent) / 'model-elsewhere'
        for env, message in (({}, 'set OXPINYIN_PROVISION_CACHE to a `tools.dev provision oracle --cache-dir` directory'),
                             ({'OXPINYIN_PROVISION_CACHE': str(self.provision.parent / 'absent')}, 'set OXPINYIN_PROVISION_CACHE'),
                             ({'OXPINYIN_PROVISION_CACHE': str(self.provision), 'PINYIN_MODEL_CACHE': str(model)},
                              f'{model / "extracted"} is not a model20 extraction; set PINYIN_MODEL_CACHE')):
            with (self.subTest(env=env), mock.patch.object(images, 'run') as run, mock.patch.object(images, 'quiet') as quiet,
                  mock.patch.object(images, 'select_runtime') as select,
                  contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(images.Finished) as raised):
                images.capture_pin(['1'], root=ROOT, env=dict(env, PATH=''))
            self.assertEqual(raised.exception.status, 2)
            self.assertIn(message, error.getvalue())
            run.assert_not_called()
            quiet.assert_not_called()
            select.assert_not_called()

    def test_foreign_architecture_and_bad_run_names_are_refused_before_any_runtime_call(self):
        for argv, machine in ((['1'], 'riscv64'), (['../1'], 'x86_64'), (['a b'], 'x86_64'), ([], 'x86_64')):
            with self.subTest(argv=argv, machine=machine), \
                    mock.patch.object(images.platform, 'machine', return_value=machine), \
                    mock.patch.object(images, 'select_runtime', return_value='docker') as select, \
                    mock.patch.object(images, 'run') as run, mock.patch.object(images, 'quiet') as quiet, \
                    contextlib.redirect_stderr(io.StringIO()), self.assertRaises(images.Finished) as raised:
                images.capture_pin(argv, root=ROOT, env={'PATH': ''})
            self.assertEqual(raised.exception.status, 2)
            run.assert_not_called()
            quiet.assert_not_called()
            select.assert_not_called()

    def test_the_run_user_is_the_invoking_uid_and_none_without_one(self):
        posix = types.SimpleNamespace(getuid=lambda: 1000, getgid=lambda: 1001)
        with mock.patch.object(images, 'os', posix):
            self.assertEqual(images.run_user(), ['--user', '1000:1001'])
        with mock.patch.object(images, 'os', types.SimpleNamespace()):
            self.assertEqual(images.run_user(), [])

if __name__ == '__main__':
    unittest.main()
