"""Loader fixtures and launch regressions; no oracle or system install needed."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import runtime


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
        script = Path(runtime.__file__).with_suffix('.sh')
        command = ['bash', '-c', 'source "$1"; tkrzw_run "${PINYIN_ORACLE_DBM:-bdb}" subject subject.so sh -c \'echo MATCH\'',
                   str(script.parent / 'unlabelled-runner'), str(script)]
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

    def test_shell_guard_needs_no_uv_or_python(self):
        script = Path(runtime.__file__).with_suffix('.sh')
        self.env['PATH'] = '/usr/bin:/bin'
        result = subprocess.run(['bash', str(script), 'tkrzw', 'image-runner', 'subject',
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
        script = Path(runtime.__file__).with_suffix('.sh')
        result = subprocess.run(['bash', str(script), 'bdb', 'linked-worker', 'pin',
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
        script = Path(runtime.__file__).with_suffix('.sh')
        command = ['bash', '-c', 'source "$1"; tkrzw_run tkrzw pin subject.so sh -c \'printf "%s" "$LD_LIBRARY_PATH"\'',
                   str(script.parent / 'fixture-runner'), str(script)]
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
        script = Path(runtime.__file__).with_suffix('.sh')
        for cell in ('bdb', 'kc'):
            result = subprocess.run(['bash', '-c', 'source "$1"; tkrzw_run --quiet-stderr "$2" subject /bin/sh sh -c \'echo RESULT; echo diagnostic >&2\'',
                                     str(script.parent / 'fixture-runner'), str(script), cell], env=self.env, capture_output=True, text=True)
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
        script = Path(runtime.__file__).with_suffix('.sh')
        for name in ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH'):
            with self.subTest(name=name):
                result = subprocess.run(
                    ['bash', '-c',
                     f'{name}=/tmp/override; source "$1"; _tkrzw_check runner subject /bin/true',
                     str(script.parent / 'fixture'), str(script)],
                    capture_output=True, text=True)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('REFUSED: runner: subject:', result.stderr)
                self.assertIn('otool -L recorded install name', result.stderr)


if __name__ == '__main__':
    unittest.main()
