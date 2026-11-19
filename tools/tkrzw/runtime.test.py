"""Host-loader fixtures for runtime identity, refusal and launch behavior."""
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev.cell import CellError, Settings
from tools.dev.drivers import DriverServices
from tools.tkrzw import runtime


# The fixtures are /bin/sh stubs for uname and ldd: Unix loaders only.
if sys.platform != 'win32':
    class AdapterControlTests(unittest.TestCase):
        def setUp(self):
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            # Darwin's /var temporary root aliases /private/var; loader identities
            # are physical paths on every host, just like the retained ELF fixtures.
            self.directory = Path(temporary.name).resolve()
            self.library = self.directory / 'libtkrzw.so.1'
            self.library.write_bytes(b'verified')
            self.other = self.directory / 'other.so'
            self.other.write_bytes(b'other')
            for name, program in {
                'uname': 'printf "%s\\n" "${FAKE_HOST:-Linux}"',
                'ldd': 'printf "%s\\n" "$FAKE_LDD"; exit "${FAKE_STATUS:-0}"',
            }.items():
                path = self.directory / name
                path.write_text('#!/bin/sh\n' + program + '\n')
                path.chmod(0o755)
            self.env = dict(os.environ, PATH=str(self.directory) + os.pathsep + os.environ['PATH'],
                            OXPINYIN_TKRZW_LIBRARY=str(self.library), FAKE_LDD=f'libtkrzw.so.1 => {self.library} (0x1234)')
            for name in ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH', 'OXPINYIN_TKRZW_PREFIX', 'OXPINYIN_TKRZW_PREFIXES'):
                self.env.pop(name, None)

        def invoke(self, args, **settings):
            env = dict(self.env, **settings)
            done = subprocess.run([sys.executable, '-m', 'tools.tkrzw.runtime', *map(str, args)],
                                  cwd=ROOT, env=env, capture_output=True, timeout=10)
            return done

        def test_advisory_labels_never_bypass_a_loaded_library(self):
            for cell in ('bdb', 'kc', 'tkrzw', 'unknown'):
                done = self.invoke(['--print-path', cell, 'fixture', 'pin', '/object'])
                self.assertEqual(done.returncode, 0)
                self.assertEqual(done.stdout, (str(self.library) + '\n').encode())

        def test_absent_library_is_allowed_for_every_cell(self):
            done = self.invoke(['--print-path', 'tkrzw', 'fixture', 'pin', '/object'], FAKE_LDD='libc.so => /lib/libc.so (0x1)')
            self.assertEqual((done.returncode, done.stdout, done.stderr), (0, b'', b''))

        def test_wrong_missing_unresolved_and_failed_libraries_are_refused(self):
            cases = [(f'libtkrzw.so.1 => {self.other} (0x1)', '0'),
                     ('libtkrzw.so.1 => not found', '0'), ('libtkrzw.so.1 unresolved', '0'),
                     ('', '1'), (f'libtkrzw.so.1 => {self.library} (0x1)', '1')]
            for output, status in cases:
                with self.subTest(output=output, status=status):
                    done = self.invoke(['bdb', 'fixture', 'subject', '/object'], FAKE_LDD=output, FAKE_STATUS=status)
                    self.assertEqual(done.returncode, 1)
                    self.assertIn(b'REFUSED: fixture: subject:', done.stderr)

        def test_aliases_are_one_library_but_distinct_libraries_are_ambiguous(self):
            alias = self.directory / 'alias.so'
            alias.symlink_to(self.library)
            for path, expected in ((alias, 0), (self.other, 1)):
                output = self.env['FAKE_LDD'] + f'\nlibtkrzw.so.1 => {path} (0x2)'
                self.assertEqual(self.invoke(['bdb', 'fixture', 'L', '/object'], FAKE_LDD=output).returncode, expected)

        def test_expected_library_then_prefix_then_first_prefix_precedence(self):
            prefix = self.directory / 'prefix'
            (prefix / 'lib').mkdir(parents=True)
            (prefix / 'lib/libtkrzw.so.1').symlink_to(self.library)
            for key in ('OXPINYIN_TKRZW_PREFIX', 'OXPINYIN_TKRZW_PREFIXES'):
                env = dict(self.env)
                env.pop('OXPINYIN_TKRZW_LIBRARY')
                env[key] = str(prefix) + (':/ignored' if key.endswith('PREFIXES') else '')
                self.assertEqual(runtime.check('bdb', 'fixture', 'pin', '/object', env=env), self.library)
            done = self.invoke(['bdb', 'fixture', 'pin', '/object'], OXPINYIN_TKRZW_PREFIX='/ignored')
            self.assertEqual(done.returncode, 0)
            done = self.invoke(['bdb', 'fixture', 'pin', '/object'], OXPINYIN_TKRZW_LIBRARY='')
            self.assertEqual(done.returncode, 1)

        def test_relative_loader_entries_keep_the_callers_cwd(self):
            env = dict(self.env, FAKE_LDD='libtkrzw.so.1 => ./libtkrzw.so.1 (0x1)', OXPINYIN_TKRZW_LIBRARY='./libtkrzw.so.1')
            self.assertEqual(runtime.check('kc', 'fixture', 'pin', 'object', env=env, cwd=self.directory), self.library)

        def test_child_status_and_quiet_stderr(self):
            for code in (0, 7, 77, 127):
                for quiet in ([], ['--quiet-stderr']):
                    done = self.invoke([*quiet, 'bdb', 'fixture', 'pin', '/object', '--',
                                        'bash', '-c', f'printf out; printf err >&2; exit {code}'])
                    self.assertEqual(done.returncode, code)
                    self.assertEqual(done.stdout, b'out')
                    self.assertEqual(done.stderr, b'' if quiet else b'err')

        def test_missing_command_and_usage_status(self):
            self.assertEqual(self.invoke([]).returncode, 2)
            self.assertEqual(self.invoke(['bdb', 'fixture', 'pin', '/object', '--', 'not-a-real-command']).returncode, 1)

        def test_scripts_launch_their_interpreter_and_other_files_themselves(self):
            elf = self.directory / 'interpreter'
            elf.write_bytes(b'\x7fELF' + b'payload')
            script = self.directory / 'wrapper'
            script.write_text(f'#!{elf} -e\nexec "$0.bin" "$@"\n')
            nested = self.directory / 'nested'
            nested.write_text(f'#! {script}\n')
            relative = self.directory / 'relative'
            relative.write_text('#!interpreter\n')
            self.assertEqual(runtime.launched(script), elf)
            self.assertEqual(runtime.launched(nested), elf)
            self.assertEqual(runtime.launched(relative, self.directory), elf)
            # Anything that is not a script, or a chain no kernel follows, is
            # checked as itself, so the loader check still refuses what it cannot read.
            loop = self.directory / 'loop'
            loop.write_text(f'#!{loop}\n')
            plain = self.directory / 'plain'
            plain.write_text('exec true\n')
            unreadable = self.directory / 'execute-only'
            unreadable.write_bytes(b'\x7fELF')
            unreadable.chmod(0o111)
            fat = self.directory / 'universal'
            fat.write_bytes(bytes.fromhex('cafebabe') + b'payload')
            for path in (elf, fat, plain, loop, unreadable):
                self.assertEqual(runtime.launched(path), path)
            orphan = self.directory / 'orphan'
            orphan.write_text(f'#!{self.directory / "missing"}\n')
            for path in (self.directory / 'missing', self.directory, orphan):
                self.assertIsNone(runtime.launched(path))


if sys.platform == 'win32':
    class WindowsAdapterTests(unittest.TestCase):
        """Windows has no uname, ldd or otool: the adapter refuses and never guesses a library."""

        def invoke(self, args):
            with tempfile.TemporaryDirectory() as empty:
                return subprocess.run([sys.executable, '-m', 'tools.tkrzw.runtime', *map(str, args)],
                                      cwd=ROOT, env=dict(os.environ, PATH=empty), capture_output=True,
                                      text=True, timeout=10)

        def test_a_missing_host_loader_command_is_refused_with_the_operating_system_error(self):
            with self.assertRaises(FileNotFoundError) as missing:
                subprocess.run(['no-such-loader-command'], capture_output=True)
            done = self.invoke(['bdb', 'fixture', 'pin', 'object'])
            self.assertEqual((done.returncode, done.stdout), (1, ''))
            self.assertEqual(done.stderr.strip(),
                             f'REFUSED: fixture: pin: libtkrzw resolved to <unknown>: {missing.exception}')


if sys.platform.startswith('linux'):
    class LoaderTests(unittest.TestCase):
        def setUp(self):
            self.scratch = tempfile.TemporaryDirectory()
            self.addCleanup(self.scratch.cleanup)
            self.root = Path(self.scratch.name)
            self.cache = self.root / 'cache'
            self.system = self.root / 'system'
            self.cwd = self.root / 'driver-cwd'
            self.cwd.mkdir()
            source = self.root / 'dependency.c'
            source.write_text('int dependency(void) { return 1064; }\n')
            for prefix in (self.cache, self.system):
                lib = prefix / 'lib'
                lib.mkdir(parents=True)
                # A distinct SONAME makes the missing fixture independent of
                # whether the real host happens to have a packaged libtkrzw.
                subprocess.run(['cc', '-shared', '-fPIC', str(source),
                                '-Wl,-soname,libtkrzw.so.1064', '-o', str(lib / 'libtkrzw.so.1064')], check=True)
                (lib / 'libtkrzw.so.1').symlink_to('libtkrzw.so.1064')
            subject = self.root / 'subject.c'
            subject.write_text('extern int dependency(void); int subject(void) { return dependency(); }\n')
            self.obj = self.cwd / 'subject.so'
            subprocess.run(['cc', '-shared', '-fPIC', str(subject), '-L' + str(self.cache / 'lib'),
                            '-l:libtkrzw.so.1064', '-o', str(self.obj)], check=True)
            self.env = dict(os.environ, OXPINYIN_TKRZW_PREFIX=str(self.cache),
                            LD_LIBRARY_PATH=str(self.cache / 'lib'))

        def check(self, **kw):
            return runtime.check('tkrzw', 'fixture-runner', 'pin', 'subject.so',
                                 env=self.env, cwd=self.cwd, **kw)

        def test_cache_is_accepted_with_exact_cwd_and_relative_search_path(self):
            self.env['LD_LIBRARY_PATH'] = '../cache/lib'
            self.assertEqual(self.check(), (self.cache / 'lib/libtkrzw.so.1064').resolve())

        def test_system_library_is_refused_with_runner_side_and_found_path(self):
            self.env['LD_LIBRARY_PATH'] = str(self.system / 'lib')
            with self.assertRaisesRegex(runtime.RuntimeRefused, r'fixture-runner: pin:.*system/lib/libtkrzw.so.1064'):
                self.check()

        def test_other_cell_labels_cannot_bypass_tkrzw_subject(self):
            self.env['LD_LIBRARY_PATH'] = str(self.system / 'lib')
            for cell in ('bdb', 'kc', 'tkrzw', 'auto', ''):
                with self.subTest(cell=cell), self.assertRaisesRegex(runtime.RuntimeRefused, 'system/lib/libtkrzw.so.1064'):
                    runtime.check(cell, 'labelled-runner', 'subject', self.obj,
                                  env=self.env, cwd=self.cwd)

        def test_direct_guard_needs_no_uv_with_selected_interpreter(self):
            script = Path(runtime.__file__)
            self.env['PATH'] = '/usr/bin:/bin'
            result = subprocess.run([sys.executable, str(script), 'tkrzw', 'image-runner', 'subject',
                                     str(self.obj), '--', '/bin/sh', '-c', 'echo RESULT'],
                                    cwd=self.cwd, env=self.env, capture_output=True, text=True)
            self.assertEqual((result.returncode, result.stdout, result.stderr), (0, 'RESULT\n', ''))

        def linked_worker(self):
            """A plain dlopen subject and a worker linked against libtkrzw."""
            plain = self.cwd / 'plain.so'
            subprocess.run(['cc', '-shared', '-fPIC', str(self.root / 'dependency.c'),
                            '-o', str(plain)], check=True)
            source = self.root / 'worker.c'
            source.write_text('extern int dependency(void); int main(void) { return dependency() != 1064; }\n')
            worker = self.cwd / 'worker'
            subprocess.run(['cc', str(source), '-L' + str(self.cache / 'lib'),
                            '-l:libtkrzw.so.1064', '-o', str(worker)], check=True)
            return plain, worker

        def test_linked_worker_is_checked_even_when_dlopen_subject_has_no_tkrzw(self):
            plain, worker = self.linked_worker()
            self.env['LD_LIBRARY_PATH'] = str(self.system / 'lib')
            script = Path(runtime.__file__)
            result = subprocess.run([sys.executable, str(script), 'bdb', 'linked-worker', 'pin',
                                     str(plain), '--', str(worker)], cwd=self.cwd, env=self.env,
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, '')
            self.assertIn('linked-worker: pin:', result.stderr)
            self.assertIn(str(self.system / 'lib/libtkrzw.so.1064'), result.stderr)
            self.assertIn(str(worker), result.stderr)

        def test_script_launcher_runs_with_its_interpreter_and_subject_checked(self):
            # Debian's valgrind is a sh wrapper that execs valgrind.bin; ldd cannot read it.
            launcher = self.cwd / 'launcher'
            launcher.write_text(f'#!/bin/sh -e\nexec {shlex.quote(sys.executable)} "$@"\n')
            launcher.chmod(0o755)

            def run(command, obj):
                return DriverServices(Settings.read(self.env)).run(
                    [str(command), '-c', 'print("launched")'], obj, runner='script-launcher',
                    side='subject', cwd=self.cwd)
            result = run(launcher, self.obj)
            self.assertEqual((result.returncode, result.stdout), (0, b'launched\n'))
            self.env['LD_LIBRARY_PATH'] = str(self.system / 'lib')
            with self.assertRaisesRegex(CellError, r'REFUSED: script-launcher: subject:.*system/lib/libtkrzw.so.1064'
                                        rf'.*object {re.escape(str(self.obj))},'):
                run(launcher, self.obj)
            plain, worker = self.linked_worker()
            interpreted = self.cwd / 'interpreted'
            interpreted.write_text(f'#!{worker}\n')
            interpreted.chmod(0o755)
            with self.assertRaisesRegex(CellError, r'REFUSED: script-launcher: subject:.*system/lib/libtkrzw.so.1064'
                                        rf'.*object {re.escape(str(worker))},'):
                run(interpreted, plain)
            result = subprocess.run([sys.executable, str(Path(runtime.__file__)), 'bdb', 'script-launcher', 'pin',
                                     str(plain), '--', str(interpreted)], cwd=self.cwd, env=self.env,
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stdout, '')
            self.assertIn(f'REFUSED: script-launcher: pin: libtkrzw resolved to {self.system}/lib/', result.stderr)
            self.assertIn(f'(object {worker},', result.stderr)

        def test_an_unconfigured_cache_is_not_implicitly_trusted(self):
            self.env.pop('OXPINYIN_TKRZW_PREFIX', None)
            self.env.pop('OXPINYIN_TKRZW_PREFIXES', None)
            self.env.pop('OXPINYIN_TKRZW_LIBRARY', None)
            with self.assertRaisesRegex(runtime.RuntimeRefused, 'unconfigured verified runtime'):
                self.check()

        def test_verified_host_without_cache_accepts_only_exact_library(self):
            self.env.pop('OXPINYIN_TKRZW_PREFIX', None)
            self.env.pop('OXPINYIN_TKRZW_PREFIXES', None)
            expected = (self.system / 'lib/libtkrzw.so.1064').resolve()
            self.env['OXPINYIN_TKRZW_LIBRARY'] = str(expected)
            self.env['LD_LIBRARY_PATH'] = str(self.system / 'lib')
            self.assertEqual(runtime.check('auto', 'host-runner', 'subject', self.obj,
                                           env=self.env, cwd=self.cwd), expected)
            self.env['LD_LIBRARY_PATH'] = str(self.cache / 'lib')
            with self.assertRaisesRegex(runtime.RuntimeRefused, 'cache/lib/libtkrzw.so.1064'):
                runtime.check('auto', 'host-runner', 'subject', self.obj, env=self.env, cwd=self.cwd)

        def transitive_subject(self):
            bridge_source = self.root / 'bridge.c'
            bridge_source.write_text('extern int dependency(void); int bridge(void) { return dependency(); }\n')
            bridge = self.cwd / 'libbridge-1064.so'
            subprocess.run(['cc', '-shared', '-fPIC', str(bridge_source),
                            '-L' + str(self.cache / 'lib'), '-l:libtkrzw.so.1064',
                            '-Wl,-soname,libbridge-1064.so', '-o', str(bridge)], check=True)
            source = self.root / 'transitive.c'
            source.write_text('extern int bridge(void); int subject(void) { return bridge(); }\n')
            obj = self.cwd / 'transitive.so'
            subprocess.run(['cc', '-shared', '-fPIC', str(source), '-L' + str(self.cwd),
                            '-l:libbridge-1064.so', '-o', str(obj)], check=True)
            needed = subprocess.check_output(['readelf', '-d', str(obj)], text=True)
            self.assertIn('[libbridge-1064.so]', needed)
            self.assertNotIn('[libtkrzw.so.1064]', needed)
            return obj

        def test_transitive_cache_dependency_is_accepted(self):
            obj = self.transitive_subject()
            self.env['LD_LIBRARY_PATH'] = str(self.cwd) + ':' + str(self.cache / 'lib')
            self.assertEqual(runtime.check('auto', 'transitive-runner', 'subject', obj,
                                           env=self.env, cwd=self.cwd),
                             (self.cache / 'lib/libtkrzw.so.1064').resolve())

        def test_transitive_system_dependency_is_refused(self):
            obj = self.transitive_subject()
            self.env['LD_LIBRARY_PATH'] = str(self.cwd) + ':' + str(self.system / 'lib')
            with self.assertRaisesRegex(runtime.RuntimeRefused,
                                        r'transitive-runner: subject:.*system/lib/libtkrzw.so.1064'):
                runtime.check('auto', 'transitive-runner', 'subject', obj,
                              env=self.env, cwd=self.cwd)

        def test_transitive_missing_dependency_is_refused(self):
            obj = self.transitive_subject()
            self.env['LD_LIBRARY_PATH'] = str(self.cwd)
            with self.assertRaisesRegex(runtime.RuntimeRefused, 'not found'):
                runtime.check('auto', 'transitive-runner', 'subject', obj,
                              env=self.env, cwd=self.cwd)

        def test_automatic_non_tkrzw_object_does_not_require_a_cache(self):
            plain = self.cwd / 'plain.so'
            subprocess.run(['cc', '-shared', '-fPIC', str(self.root / 'dependency.c'),
                            '-o', str(plain)], check=True)
            self.env['OXPINYIN_TKRZW_PREFIX'] = str(self.root / 'missing-cache')
            self.assertIsNone(runtime.check('auto', 'non-tkrzw-runner', 'subject', plain,
                                            env=self.env, cwd=self.cwd))

        def test_system_preload_is_refused_even_with_the_cache_search_path(self):
            self.env['LD_PRELOAD'] = str(self.system / 'lib/libtkrzw.so.1064')
            with self.assertRaisesRegex(runtime.RuntimeRefused, 'system/lib/libtkrzw.so.1064'):
                self.check()

        def test_cache_preload_is_accepted(self):
            self.env['LD_PRELOAD'] = str(self.cache / 'lib/libtkrzw.so.1064')
            self.assertEqual(self.check(), (self.cache / 'lib/libtkrzw.so.1064').resolve())

        def test_refusal_prevents_command_and_result_output(self):
            marker = self.root / 'launched'
            self.env['LD_LIBRARY_PATH'] = str(self.system / 'lib')
            command = [sys.executable, '-B', str(Path(runtime.__file__)), 'tkrzw',
                       'fixture-runner', 'subject', 'subject.so', '--', sys.executable,
                       '-c', f'from pathlib import Path; Path({str(marker)!r}).touch(); print("MATCH")']
            result = subprocess.run(command, cwd=self.cwd, env=self.env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(marker.exists())
            self.assertEqual(result.stdout, '')
            self.assertIn('fixture-runner: subject:', result.stderr)
            self.assertIn(str(self.system / 'lib/libtkrzw.so.1064'), result.stderr)

        def test_shell_and_python_share_the_launch_environment(self):
            script = Path(runtime.__file__)
            command = [sys.executable, str(script), 'tkrzw', 'fixture-runner', 'pin', 'subject.so',
                       '--', 'sh', '-c', 'printf "%s" "$LD_LIBRARY_PATH"']
            result = subprocess.run(command, cwd=self.cwd, env=self.env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, self.env['LD_LIBRARY_PATH'])

        def test_quiet_driver_stderr_does_not_hide_a_refusal(self):
            command = [sys.executable, '-B', str(Path(runtime.__file__)), '--quiet-stderr',
                       'tkrzw', 'fixture-runner', 'pin', 'subject.so', '--',
                       'sh', '-c', 'echo RESULT; echo driver-stderr >&2']
            accepted = subprocess.run(command, cwd=self.cwd, env=self.env, capture_output=True, text=True)
            self.assertEqual((accepted.returncode, accepted.stdout, accepted.stderr), (0, 'RESULT\n', ''))
            self.env['LD_LIBRARY_PATH'] = str(self.system / 'lib')
            refused = subprocess.run(command, cwd=self.cwd, env=self.env, capture_output=True, text=True)
            self.assertNotEqual(refused.returncode, 0)
            self.assertEqual(refused.stdout, '')
            self.assertIn('REFUSED: fixture-runner: pin:', refused.stderr)


if sys.platform == 'darwin':
    class DarwinLoaderTests(unittest.TestCase):
        def setUp(self):
            directory = tempfile.TemporaryDirectory()
            self.addCleanup(directory.cleanup)
            self.root = Path(directory.name).resolve()
            self.verified = self.root / 'verified/libtkrzw.fixture.dylib'
            self.wrong = self.root / 'wrong/libtkrzw.fixture.dylib'
            self.env = {name: value for name, value in os.environ.items() if not name.startswith('DYLD_')}
            self.env['OXPINYIN_TKRZW_LIBRARY'] = str(self.verified)

        def build(self, obj, source, *flags):
            obj.parent.mkdir(parents=True, exist_ok=True)
            code = obj.with_suffix('.c')
            code.write_text(source + '\n')
            subprocess.run(['cc', str(code), *map(str, flags), '-o', str(obj)], env=self.env, check=True)

        def build_dependency(self, library, name=None):
            value = 1 if library == self.verified else 2
            self.build(library, f'int dependency(void) {{ return {value}; }}',
                       '-dynamiclib', '-install_name', name or str(library))

        def build_bridge(self, library, name, *flags):
            bridge = self.root / 'bridge.dylib'
            self.build(bridge, 'extern int dependency(void); int bridge(void) { return dependency(); }',
                       '-dynamiclib', '-install_name', name, *flags, library)
            return bridge

        def transitive_subject(self, loaded, install_name=None, *bridge_flags):
            for library in (self.verified, self.wrong):
                self.build_dependency(library, install_name if library == loaded else None)
            bridge = self.build_bridge(loaded, '@loader_path/bridge.dylib', *bridge_flags)
            subject = self.root / 'subject.dylib'
            self.build(subject, 'extern int bridge(void); int subject(void) { return bridge(); }',
                       '-dynamiclib', bridge)
            direct = subprocess.check_output(['otool', '-L', str(subject)], text=True)
            self.assertIn('@loader_path/bridge.dylib', direct)
            self.assertNotIn('libtkrzw', direct)
            return subject

        def check(self, subject):
            return runtime.check('auto', 'transitive-runner', 'subject', subject.relative_to(self.root),
                                 env=self.env, cwd=self.root)

        def test_transitive_verified_dependency_is_accepted(self):
            subject = self.transitive_subject(self.verified, '@loader_path/verified/libtkrzw.fixture.dylib')
            self.assertEqual(self.check(subject), self.verified)

        def test_transitive_wrong_dependency_is_refused(self):
            subject = self.transitive_subject(self.wrong)
            with self.assertRaisesRegex(runtime.RuntimeRefused, r'transitive-runner: subject:.*wrong/libtkrzw\.fixture\.dylib'):
                self.check(subject)

        def test_transitive_missing_dependency_is_refused(self):
            subject = self.transitive_subject(self.wrong)
            self.wrong.unlink()
            with self.assertRaisesRegex(runtime.RuntimeRefused, r'libtkrzw\.fixture\.dylib'):
                self.check(subject)

        def test_transitive_executable_relative_dependency_is_refused(self):
            relative = os.path.relpath(self.verified, Path(sys.executable).resolve().parent)
            subject = self.transitive_subject(self.verified, '@executable_path/' + relative)
            # The probe's own executable can resolve the verified library.
            subprocess.run([sys.executable, '-I', '-S', '-c',
                            'import ctypes, sys; ctypes.CDLL(sys.argv[1])', str(subject)],
                           env=self.env, cwd=self.root, check=True)
            with self.assertRaisesRegex(runtime.RuntimeRefused, r'main executable.*@executable_path'):
                self.check(subject)

        def test_transitive_executable_relative_runpath_is_refused(self):
            relative = os.path.relpath(self.verified.parent, Path(sys.executable).resolve().parent)
            install_name = '@rpath/' + self.verified.name
            subject = self.transitive_subject(self.verified, install_name, '-rpath', '@executable_path/' + relative)
            subprocess.run([sys.executable, '-I', '-S', '-c',
                            'import ctypes, sys; ctypes.CDLL(sys.argv[1])', str(subject)],
                           env=self.env, cwd=self.root, check=True)
            with self.assertRaisesRegex(runtime.RuntimeRefused, r'main executable.*@rpath'):
                self.check(subject)

        def test_dylib_without_tkrzw_is_accepted(self):
            subject = self.root / 'plain.dylib'
            self.build(subject, 'int subject(void) { return 1; }', '-dynamiclib')
            self.assertIsNone(self.check(subject))

        def worker(self, dependency, symbol='dependency', *flags):
            worker = self.root / 'bin/worker'
            self.build(worker, f'''
#include <stdio.h>
extern int {symbol}(void);
int main(void) {{
    FILE *marker = fopen("launched", "w");
    if (!marker) return 99;
    fclose(marker);
    return {symbol}();
}}''', dependency, *flags)
            return worker

        def test_executable_accepts_verified_direct_import_without_running_main(self):
            self.build_dependency(self.verified)
            self.assertEqual(self.check(self.worker(self.verified)), self.verified)
            self.assertFalse((self.root / 'launched').exists())

        def test_executable_accepts_verified_import_through_bridge(self):
            self.build_dependency(self.verified, '@loader_path/verified/' + self.verified.name)
            bridge = self.build_bridge(self.verified, '@loader_path/../bridge.dylib')
            worker = self.worker(bridge, 'bridge')
            direct = subprocess.check_output(['otool', '-L', str(worker)], text=True)
            self.assertIn('@loader_path/../bridge.dylib', direct)
            self.assertNotIn('libtkrzw', direct)
            self.assertEqual(self.check(worker), self.verified)
            self.assertFalse((self.root / 'launched').exists())

        def test_executable_refuses_wrong_transitive_import(self):
            self.build_dependency(self.verified)
            self.build_dependency(self.wrong)
            bridge = self.build_bridge(self.wrong, '@loader_path/../bridge.dylib')
            with self.assertRaisesRegex(runtime.RuntimeRefused, r'wrong/libtkrzw\.fixture\.dylib'):
                self.check(self.worker(bridge, 'bridge'))

        def test_executable_relative_imports_match_dyld(self):
            self.build_dependency(self.verified, '@executable_path/../verified/' + self.verified.name)
            bridge = self.build_bridge(self.verified, '@executable_path/../bridge.dylib')
            worker = self.worker(bridge, 'bridge')
            self.assertEqual(self.check(worker), self.verified)
            self.assertFalse((self.root / 'launched').exists())
            self.assertEqual(subprocess.run([worker], cwd=self.root, env=self.env).returncode, 1)

        def test_executable_runpath_stack_matches_dyld(self):
            self.build_dependency(self.verified, '@rpath/' + self.verified.name)
            for owner in ('executable', 'bridge'):
                with self.subTest(owner=owner):
                    (self.root / 'launched').unlink(missing_ok=True)
                    bridge_flags = ('-rpath', '@loader_path/verified') if owner == 'bridge' else ()
                    main_flags = ('-rpath', '@executable_path/../verified') if owner == 'executable' else ()
                    bridge = self.build_bridge(self.verified, '@loader_path/../bridge.dylib', *bridge_flags)
                    worker = self.worker(bridge, 'bridge', *main_flags)
                    self.assertEqual(self.check(worker), self.verified)
                    self.assertFalse((self.root / 'launched').exists())
                    self.assertEqual(subprocess.run([worker], cwd=self.root, env=self.env).returncode, 1)

        def test_executable_refuses_an_unresolvable_import(self):
            self.build_dependency(self.verified, '@rpath/' + self.verified.name)
            worker = self.worker(self.verified, 'dependency', '-rpath', '@executable_path/../missing')
            with self.assertRaisesRegex(runtime.RuntimeRefused, r'no candidate for @rpath/libtkrzw'):
                self.check(worker)

        def test_executable_refuses_multiple_runpath_candidates(self):
            for library in (self.verified, self.wrong):
                self.build_dependency(library, '@rpath/' + library.name)
            worker = self.worker(self.verified, 'dependency',
                                 '-rpath', '@executable_path/../verified',
                                 '-rpath', '@executable_path/../wrong')
            with self.assertRaisesRegex(runtime.RuntimeRefused, r'multiple candidates for @rpath/libtkrzw'):
                self.check(worker)

        def test_loader_diagnostics_do_not_block_certification(self):
            self.build_dependency(self.verified)
            worker = self.worker(self.verified)
            self.env['DYLD_PRINT_LIBRARIES'] = '1'
            self.env['DYLD_BIND_AT_LAUNCH'] = '1'
            self.assertEqual(self.check(worker), self.verified)

        def test_tkrzw_so_refuses_both_dyld_overrides(self):
            script = Path(runtime.__file__)
            for name in ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH'):
                with self.subTest(name=name):
                    result = subprocess.run(
                        [sys.executable, str(script), '--resolve', '/bin/true'],
                        capture_output=True, text=True, env=dict(os.environ, **{name: '/tmp/override'}))
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn('REFUSED:', result.stderr)
                    self.assertIn(name, result.stderr)


if __name__ == '__main__':
    unittest.main()
