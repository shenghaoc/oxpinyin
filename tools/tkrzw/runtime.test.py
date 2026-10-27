#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Host-loader fixtures, optionally compared with the preserved migration base."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.tkrzw import runtime


class AdapterControlTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
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

    def invoke(self, args, **settings):
        env = dict(self.env, **settings)
        done = subprocess.run([sys.executable, '-m', 'tools.tkrzw.runtime', *map(str, args)],
                              cwd=ROOT, env=env, capture_output=True, timeout=10)
        base = os.environ.get('U3_BASE')
        if base:
            old = subprocess.run(['bash', str(Path(base).resolve() / 'tools/tkrzw/runtime.sh'), *map(str, args)],
                                 cwd=ROOT, env=env, capture_output=True, timeout=10)
            self.assertEqual((done.returncode, done.stdout, done.stderr),
                             (old.returncode, old.stdout, old.stderr))
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

    def test_darwin_install_name_and_override_refusals(self):
        output = f'\t{self.library} (compatibility version 1.0.0, current version 1.0.0)'
        # The install-name parser requires .dylib as the shell did.
        dylib = self.directory / 'libtkrzw.dylib'
        dylib.symlink_to(self.library)
        output = f'\t{dylib} (compatibility version 1.0.0, current version 1.0.0)'
        self.assertEqual(self.invoke(['kc', 'fixture', 'oracle', '/object'], FAKE_HOST='Darwin', FAKE_LDD=output).returncode, 0)
        for variable in ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH'):
            self.assertEqual(self.invoke(['bdb', 'fixture', 'oracle', '/object'], FAKE_HOST='Darwin', **{variable: '/override'}).returncode, 1)

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

    def test_binary_qualification_covers_elf_and_macho(self):
        for magic in (b'\x7fELF', bytes.fromhex('cffaedfe'), bytes.fromhex('cefaedfe'), bytes.fromhex('feedfacf'), bytes.fromhex('feedface')):
            path = self.directory / 'object'
            path.write_bytes(magic + b'payload')
            self.assertTrue(runtime.binary(path))
        path.write_text('#!/bin/sh\n')
        self.assertFalse(runtime.binary(path))


@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux ELF loader fixtures')
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

    def test_missing_library_is_refused(self):
        self.env['LD_LIBRARY_PATH'] = ''
        with self.assertRaisesRegex(runtime.RuntimeRefused, r'fixture-runner: pin:.*not found'):
            self.check()

    def test_inherited_search_path_does_not_define_the_trusted_cache(self):
        self.env['LD_LIBRARY_PATH'] = str(self.system / 'lib')
        with self.assertRaises(runtime.RuntimeRefused):
            self.check()

    def test_symlink_alias_to_the_provisioned_object_is_accepted(self):
        alias = self.root / 'alias'
        alias.symlink_to(self.cache / 'lib', target_is_directory=True)
        self.env['LD_LIBRARY_PATH'] = str(alias)
        self.assertEqual(self.check(), (self.cache / 'lib/libtkrzw.so.1064').resolve())

    def test_other_cells_without_tkrzw_need_no_trusted_runtime(self):
        for cell in ('bdb', 'kc'):
            self.assertIsNone(runtime.check(cell, 'fixture', 'subject', '/bin/sh', env={}))

    def test_automatic_cell_selection_checks_tkrzw_objects(self):
        self.env['LD_LIBRARY_PATH'] = str(self.system / 'lib')
        with self.assertRaisesRegex(runtime.RuntimeRefused, 'system/lib/libtkrzw.so.1064'):
            runtime.check('auto', 'artifact-selected-runner', 'subject', self.obj,
                          env=self.env, cwd=self.cwd)

    def test_unset_cell_cannot_bypass_tkrzw_subject(self):
        self.env.pop('PINYIN_ORACLE_DBM', None)
        self.env['LD_LIBRARY_PATH'] = str(self.system / 'lib')
        script = Path(runtime.__file__)
        command = [sys.executable, str(script), self.env.get('PINYIN_ORACLE_DBM', 'bdb'),
                   'unlabelled-runner', 'subject', 'subject.so', '--', 'sh', '-c', 'echo MATCH']
        result = subprocess.run(command, cwd=self.cwd, env=self.env, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, '')
        self.assertIn('unlabelled-runner: subject:', result.stderr)
        self.assertIn(str(self.system / 'lib/libtkrzw.so.1064'), result.stderr)

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

    def test_linked_worker_is_checked_even_when_dlopen_subject_has_no_tkrzw(self):
        plain = self.cwd / 'plain.so'
        subprocess.run(['cc', '-shared', '-fPIC', str(self.root / 'dependency.c'),
                        '-o', str(plain)], check=True)
        source = self.root / 'worker.c'
        source.write_text('extern int dependency(void); int main(void) { return dependency() != 1064; }\n')
        worker = self.cwd / 'worker'
        subprocess.run(['cc', str(source), '-L' + str(self.cache / 'lib'),
                        '-l:libtkrzw.so.1064', '-o', str(worker)], check=True)
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

    def test_transitive_dlopen_subject_is_checked_before_driver_runs(self):
        obj = self.transitive_subject()
        self.env['LD_LIBRARY_PATH'] = str(self.cwd) + ':' + str(self.system / 'lib')
        # Python is a dlopen driver without its own tkrzw dependency. The
        # checked object must be the subject, including its dependency closure.
        command = [sys.executable, '-B', str(Path(runtime.__file__)), 'auto',
                   'dlopen-fixture', 'subject', str(obj), '--', sys.executable,
                   '-c', 'import ctypes, sys; ctypes.CDLL(sys.argv[1]); print("MATCH")', str(obj)]
        result = subprocess.run(command, cwd=self.cwd, env=self.env, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, '')
        self.assertIn('dlopen-fixture: subject:', result.stderr)
        self.assertIn(str(self.system / 'lib/libtkrzw.so.1064'), result.stderr)

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

    def test_other_cells_keep_their_driver_stderr_suppressed(self):
        script = Path(runtime.__file__)
        for cell in ('bdb', 'kc'):
            result = subprocess.run([sys.executable, str(script), '--quiet-stderr', cell, 'fixture-runner', 'subject',
                                     '/bin/sh', '--', 'sh', '-c', 'echo RESULT; echo diagnostic >&2'],
                                    env=self.env, capture_output=True, text=True)
            self.assertEqual((result.returncode, result.stdout, result.stderr), (0, 'RESULT\n', ''))


@unittest.skipUnless(sys.platform == 'darwin', 'Darwin dyld override refusal')
class DarwinLoaderOverrideTests(unittest.TestCase):
    def test_tkrzw_so_refuses_both_dyld_overrides(self):
        script = Path(runtime.__file__).parents[1] / 'cloud/env.sh'
        for name in ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH'):
            with self.subTest(name=name):
                result = subprocess.run(
                    ['bash', '-c', f'{name}=/tmp/override; source "$1"; tkrzw_so /bin/true',
                     'fixture', str(script)],
                    capture_output=True, text=True)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('REFUSED: otool -L reports the recorded libtkrzw install name',
                              result.stderr)
                self.assertIn('which DYLD_LIBRARY_PATH or DYLD_FALLBACK_LIBRARY_PATH can bypass',
                              result.stderr)

    def test_tkrzw_check_refuses_both_dyld_overrides(self):
        for name in ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH'):
            with self.subTest(name=name), self.assertRaisesRegex(runtime.RuntimeRefused, 'otool -L recorded install name'):
                runtime.check('tkrzw', 'runner', 'subject', '/bin/true', env=dict(os.environ, **{name: '/tmp/override'}))


if __name__ == '__main__':
    unittest.main()
