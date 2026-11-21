"""Host-loader fixtures for runtime identity and refusal: `check`, `launched` and their callers."""
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev.cell import CellError, Settings
from tools.dev.drivers import DriverServices
from tools.tkrzw import runtime


# The fixtures are /bin/sh stubs for uname, ldd and otool, and the adapter runs them: Unix loaders only.
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
                'otool': 'printf "%s\\n" "$FAKE_LDD"; exit "${FAKE_STATUS:-0}"',
            }.items():
                path = self.directory / name
                path.write_text('#!/bin/sh\n' + program + '\n')
                path.chmod(0o755)
            self.env = dict(os.environ, PATH=str(self.directory) + os.pathsep + os.environ['PATH'],
                            OXPINYIN_TKRZW_LIBRARY=str(self.library), FAKE_LDD=f'libtkrzw.so.1 => {self.library} (0x1234)')
            for name in ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH', 'OXPINYIN_TKRZW_PREFIX', 'OXPINYIN_TKRZW_PREFIXES'):
                self.env.pop(name, None)

        def check(self, cell, side, obj='/object', **settings):
            return runtime.check(cell, 'fixture', side, obj, env=dict(self.env, **settings))

        def test_advisory_labels_never_bypass_a_loaded_library(self):
            for cell in ('bdb', 'kc', 'tkrzw', 'unknown'):
                self.assertEqual(self.check(cell, 'pin'), self.library)

        def test_absent_library_is_allowed_for_every_cell(self):
            self.assertIsNone(self.check('tkrzw', 'pin', FAKE_LDD='libc.so => /lib/libc.so (0x1)'))

        def test_wrong_missing_unresolved_and_failed_libraries_are_refused(self):
            cases = [(f'libtkrzw.so.1 => {self.other} (0x1)', '0'),
                     ('libtkrzw.so.1 => not found', '0'), ('libtkrzw.so.1 unresolved', '0'),
                     ('', '1'), (f'libtkrzw.so.1 => {self.library} (0x1)', '1')]
            for output, status in cases:
                with self.subTest(output=output, status=status), \
                        self.assertRaisesRegex(runtime.RuntimeRefused, '^fixture: subject: libtkrzw resolved to '):
                    self.check('bdb', 'subject', FAKE_LDD=output, FAKE_STATUS=status)

        def test_aliases_are_one_library_but_distinct_libraries_are_ambiguous(self):
            alias = self.directory / 'alias.so'
            alias.symlink_to(self.library)
            self.assertEqual(self.check('bdb', 'L', FAKE_LDD=self.env['FAKE_LDD'] + f'\nlibtkrzw.so.1 => {alias} (0x2)'),
                             self.library)
            with self.assertRaisesRegex(runtime.RuntimeRefused, '^fixture: pin: libtkrzw resolved to .*; .*other.so'):
                self.check('bdb', 'L', FAKE_LDD=self.env['FAKE_LDD'] + f'\nlibtkrzw.so.1 => {self.other} (0x2)')

        def test_expected_library_then_prefix_then_first_prefix_precedence(self):
            prefix = self.directory / 'prefix'
            (prefix / 'lib').mkdir(parents=True)
            (prefix / 'lib/libtkrzw.so.1').symlink_to(self.library)
            for key in ('OXPINYIN_TKRZW_PREFIX', 'OXPINYIN_TKRZW_PREFIXES'):
                env = dict(self.env)
                env.pop('OXPINYIN_TKRZW_LIBRARY')
                env[key] = str(prefix) + (':/ignored' if key.endswith('PREFIXES') else '')
                self.assertEqual(runtime.check('bdb', 'fixture', 'pin', '/object', env=env), self.library)
            self.assertEqual(self.check('bdb', 'pin', OXPINYIN_TKRZW_PREFIX='/ignored'), self.library)
            with self.assertRaisesRegex(runtime.RuntimeRefused, 'unconfigured verified runtime'):
                self.check('bdb', 'pin', OXPINYIN_TKRZW_LIBRARY='')

        def test_relative_loader_entries_keep_the_callers_cwd(self):
            env = dict(self.env, FAKE_LDD='libtkrzw.so.1 => ./libtkrzw.so.1 (0x1)', OXPINYIN_TKRZW_LIBRARY='./libtkrzw.so.1')
            self.assertEqual(runtime.check('kc', 'fixture', 'pin', 'object', env=env, cwd=self.directory), self.library)

        def test_darwin_install_name_and_override_refusals(self):
            output = f'\t{self.library} (compatibility version 1.0.0, current version 1.0.0)'
            # The install-name parser requires .dylib as the shell did.
            dylib = self.directory / 'libtkrzw.dylib'
            dylib.symlink_to(self.library)
            output = f'\t{dylib} (compatibility version 1.0.0, current version 1.0.0)'
            self.assertEqual(self.check('kc', 'oracle', FAKE_HOST='Darwin', FAKE_LDD=output), self.library)
            for variable in ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH'):
                with self.subTest(variable=variable), \
                        self.assertRaisesRegex(runtime.RuntimeRefused, 'DYLD_LIBRARY_PATH or DYLD_FALLBACK_LIBRARY_PATH set'):
                    self.check('bdb', 'oracle', FAKE_HOST='Darwin', **{variable: '/override'})

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

        def test_a_missing_host_loader_command_is_refused_with_the_operating_system_error(self):
            with self.assertRaises(FileNotFoundError) as missing:
                subprocess.run(['no-such-loader-command'], capture_output=True)
            # Windows resolves a child's executable against this process's PATH, not the child
            # environment's, so the empty directory must be this process's PATH for the call.
            with tempfile.TemporaryDirectory() as empty, mock.patch.dict(os.environ, {'PATH': empty}), \
                    self.assertRaises(runtime.RuntimeRefused) as refused:
                runtime.check('bdb', 'fixture', 'pin', 'object', env=dict(os.environ), cwd=ROOT)
            self.assertEqual(str(refused.exception),
                             f'fixture: pin: libtkrzw resolved to <unknown>: {missing.exception}')


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
            with self.assertRaisesRegex(CellError, r'REFUSED: linked-worker: pin:.*system/lib/libtkrzw.so.1064'
                                        rf'.*object {re.escape(str(worker))},'):
                DriverServices(Settings.read(self.env)).run([str(worker)], plain, runner='linked-worker',
                                                           side='pin', cwd=self.cwd)

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

        def test_refusal_prevents_the_launch(self):
            marker = self.root / 'launched'
            self.env['LD_LIBRARY_PATH'] = str(self.system / 'lib')
            command = [sys.executable, '-c', f'from pathlib import Path; Path({str(marker)!r}).touch(); print("MATCH")']
            with self.assertRaisesRegex(CellError, r'REFUSED: fixture-runner: subject:.*system/lib/libtkrzw.so.1064'):
                DriverServices(Settings.read(self.env)).run(command, self.obj, runner='fixture-runner',
                                                           side='subject', cwd=self.cwd)
            self.assertFalse(marker.exists())


if sys.platform == 'darwin':
    class DarwinLoaderOverrideTests(unittest.TestCase):
        def test_tkrzw_so_refuses_both_dyld_overrides(self):
            for name in ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH'):
                with self.subTest(name=name), \
                        self.assertRaisesRegex(runtime.RuntimeRefused,
                                               'DYLD_LIBRARY_PATH or DYLD_FALLBACK_LIBRARY_PATH set.*'
                                               'a dyld override can bypass it'):
                    runtime.check('tkrzw', 'fixture', 'subject', '/bin/true',
                                  env=dict(os.environ, **{name: '/tmp/override'}))


if __name__ == '__main__':
    unittest.main()
