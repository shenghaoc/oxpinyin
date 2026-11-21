"""The Windows tkrzw build: the pin check, the nmake overrides, the install
layout and the marker that lets a prefix be reused."""
import contextlib
import io
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.tkrzw import windows

PIN = windows.pin()
MAKEFILE = '''# Makefile for Tkrzw for Windows

PKG_VERSION = 1.0.34
LIB_VERSION = 1.77.0

VCPATH = C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\Community\\VC\\Tools\\MSVC\\14.29.30133
VCINCPATH = $(VCPATH)\\include
HEADERFILES = tkrzw_lib_common.h tkrzw_dbm.h
LIBOBJFILES = tkrzw_lib_common.obj tkrzw_dbm.obj
CL = cl
CLFLAGS = /nologo \\
  /I "$(VCINCPATH)" \\
  /DNDEBUG /D_CRT_SECURE_NO_WARNINGS \\
  /D_TKRZW_PKG_VERSION="\\"$(PKG_VERSION)\\"" /D_TKRZW_LIB_VERSION="\\"$(LIB_VERSION)\\"" \\
  /O2 /EHsc /W3 /wd4244 /wd4267 /wd4334 /MT
LIBFLAGS = /nologo

tkrzw.lib : $(LIBOBJFILES)
\t$(LIB) $(LIBFLAGS) /OUT:$@ $(LIBOBJFILES)
'''
DEVELOPER = {'VCToolsInstallDir': 'C:\\VS\\VC\\Tools\\MSVC\\14.44.35207\\',
             'WindowsSdkDir': 'C:\\Kits\\10\\', 'WindowsSDKLibVersion': '10.0.26100.0\\',
             'VSCMD_ARG_TGT_ARCH': 'x64', 'PATH': ''}


class MakefileTests(unittest.TestCase):
    def test_macros_join_continuation_lines_and_keep_upstreams_quoting(self):
        self.assertEqual(windows.macro(MAKEFILE, 'PKG_VERSION'), '1.0.34')
        self.assertEqual(windows.macro(MAKEFILE, 'HEADERFILES'), 'tkrzw_lib_common.h tkrzw_dbm.h')
        self.assertEqual(windows.macro(MAKEFILE, 'CLFLAGS'),
                         '/nologo /I "$(VCINCPATH)" /DNDEBUG /D_CRT_SECURE_NO_WARNINGS '
                         '/D_TKRZW_PKG_VERSION="\\"$(PKG_VERSION)\\"" /D_TKRZW_LIB_VERSION="\\"$(LIB_VERSION)\\"" '
                         '/O2 /EHsc /W3 /wd4244 /wd4267 /wd4334 /MT')
        # CRLF makefiles join the same way.
        self.assertEqual(windows.macro(MAKEFILE.replace('\n', '\r\n'), 'CLFLAGS'), windows.macro(MAKEFILE, 'CLFLAGS'))
        with self.assertRaisesRegex(windows.RecipeError, 'defines no LINKFLAGS'):
            windows.macro(MAKEFILE, 'LINKFLAGS')

    def test_the_library_flags_switch_to_the_dll_runtime_exactly_once(self):
        flags = windows.library_flags(MAKEFILE)
        self.assertTrue(flags.endswith('/wd4334 /MD'))
        self.assertNotIn('/MT', flags)
        for broken in ('CLFLAGS = /O2\n', 'CLFLAGS = /MT /O2 /MT\n'):
            with self.subTest(text=broken), self.assertRaisesRegex(windows.RecipeError, '/MT'):
                windows.library_flags(broken)

    def test_overrides_name_the_runtime_and_the_developer_environments_tools(self):
        self.assertEqual(windows.overrides(MAKEFILE, DEVELOPER),
                         ['CLFLAGS=' + windows.library_flags(MAKEFILE),
                          'VCPATH=C:\\VS\\VC\\Tools\\MSVC\\14.44.35207',
                          'SDKPATH=' + str(Path('C:\\Kits\\10\\') / 'Lib' / '10.0.26100.0')])


class FetchTests(unittest.TestCase):
    def fake_git(self, head, failures=0):
        calls = []

        def runner(command, *, cwd, env, capture=False):
            calls.append(command[1])
            if command[1] == 'fetch' and calls.count('fetch') <= failures:
                raise windows.RecipeError('git exited with status 128')
            return head + '\n' if capture else None
        return runner, calls

    def test_the_checkout_must_be_the_pinned_revision(self):
        runner, _ = self.fake_git('0' * 40)
        with self.assertRaisesRegex(windows.RecipeError, 'refusing to build'):
            windows.fetch(Path('.'), PIN['revision'], {}, runner=runner)
        runner, calls = self.fake_git(PIN['revision'])
        windows.fetch(Path('.'), PIN['revision'], {}, runner=runner)
        self.assertEqual(calls, ['init', 'fetch', '-c', 'rev-parse'])

    def test_fetch_failures_are_retried_then_reported(self):
        runner, calls = self.fake_git(PIN['revision'], failures=2)
        with contextlib.redirect_stderr(io.StringIO()):
            windows.fetch(Path('.'), PIN['revision'], {}, runner=runner, sleep=lambda delay: None)
        self.assertEqual(calls.count('fetch'), 3)
        runner, calls = self.fake_git(PIN['revision'], failures=99)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(windows.RecipeError):
            windows.fetch(Path('.'), PIN['revision'], {}, runner=runner, delays=(1, 1), sleep=lambda delay: None)
        self.assertEqual(calls.count('fetch'), 3)


class BuildTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.work = Path(temporary.name).resolve()
        self.source = self.work / 'source'
        self.prefix = self.work / 'prefix'

    def runner(self, makefile=MAKEFILE):
        """A fake run: git lays out the pin's source tree, nmake produces the archive."""
        calls = []

        def run(command, *, cwd, env, capture=False):
            calls.append((list(map(str, command)), Path(cwd)))
            if command[0] == 'git' and command[1] == 'init':
                (self.source / 'VCMakefile').write_text(makefile)
                for header in ('tkrzw_lib_common.h', 'tkrzw_dbm.h'):
                    (self.source / header).write_text('// ' + header)
            if command[0] == 'nmake':
                (self.source / 'tkrzw.lib').write_bytes(b'archive')
            return PIN['revision'] + '\n' if capture else None
        return run, calls

    def build(self, makefile=MAKEFILE, developer=DEVELOPER):
        run, calls = self.runner(makefile)
        with mock.patch.object(windows, 'msvc_environment', side_effect=lambda env: dict(env, **developer)), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            prefix = windows.build(self.prefix, {'PATH': ''}, runner=run, source=self.source)
        return prefix, calls, output.getvalue()

    def test_nmake_builds_the_library_target_with_the_overrides_and_the_install_is_complete(self):
        prefix, calls, output = self.build()
        nmake = next(command for command, cwd in calls if command[0] == 'nmake')
        self.assertEqual(nmake, ['nmake', '/nologo', '-f', 'VCMakefile', *windows.overrides(MAKEFILE, DEVELOPER), 'tkrzw.lib'])
        self.assertEqual([cwd for command, cwd in calls if command[0] == 'nmake'], [self.source])
        self.assertEqual(prefix, self.prefix)
        self.assertEqual(sorted(path.relative_to(prefix).as_posix() for path in prefix.rglob('*') if path.is_file()),
                         ['REV', 'include/tkrzw_dbm.h', 'include/tkrzw_lib_common.h', 'lib/pkgconfig/tkrzw.pc', 'lib/tkrzw.lib'])
        self.assertEqual((prefix / 'lib/tkrzw.lib').read_bytes(), b'archive')
        self.assertEqual((prefix / 'lib/pkgconfig/tkrzw.pc').read_text(), windows.pc_text(prefix, PIN['version']))
        self.assertIn(f'prefix={prefix.as_posix()}\n', windows.pc_text(prefix, PIN['version']))
        self.assertIn('tkrzw_libdir=-L${libdir}\ntkrzw_includedir=-I${includedir}\n', windows.pc_text(prefix, PIN['version']))
        self.assertIn('Libs: "${tkrzw_libdir}" -ltkrzw\nCflags: "${tkrzw_includedir}"\n', windows.pc_text(prefix, PIN['version']))
        self.assertTrue(windows.built(prefix, PIN['revision']))
        self.assertIn('installed in', output)

    def test_a_complete_prefix_is_reused_and_an_incomplete_one_is_refused(self):
        self.build()
        run, calls = self.runner()
        with mock.patch.object(windows, 'msvc_environment', side_effect=lambda env: dict(env, **DEVELOPER)):
            with contextlib.redirect_stdout(io.StringIO()):
                windows.build(self.prefix, {'PATH': ''}, runner=run)
            self.assertEqual(calls, [])
            (self.prefix / windows.MARKER).write_text('0' * 40 + '\n')
            self.assertFalse(windows.built(self.prefix, PIN['revision']))
            with self.assertRaisesRegex(windows.RecipeError, 'not empty'):
                windows.build(self.prefix, {'PATH': ''}, runner=run)

    def test_only_an_x64_environment_builds_or_reuses_the_archive(self):
        self.build()
        for target in ('arm64', ''):
            with self.subTest(target=target), self.assertRaisesRegex(windows.RecipeError, 'not x64'):
                self.build(developer=dict(DEVELOPER, VSCMD_ARG_TGT_ARCH=target))

    def test_a_prefix_another_builder_holds_is_refused_as_busy(self):
        from tools import cachedir
        run, calls = self.runner()
        with cachedir.exclusive_fetch(self.prefix), \
                mock.patch.object(windows, 'msvc_environment', side_effect=lambda env: dict(env, **DEVELOPER)), \
                self.assertRaisesRegex(OSError, 'busy'):
            windows.build(self.prefix, {'PATH': ''}, runner=run, source=self.source)
        self.assertEqual(calls, [])
        self.assertFalse(self.prefix.exists())

    def test_a_source_whose_version_differs_from_the_pin_is_refused(self):
        with self.assertRaisesRegex(windows.RecipeError, 'not the pinned'):
            self.build(MAKEFILE.replace('PKG_VERSION = 1.0.34', 'PKG_VERSION = 1.0.35'))
        self.assertFalse(self.prefix.exists() and any(self.prefix.iterdir()))


class MetadataTests(unittest.TestCase):
    def test_a_prefix_with_a_space_stays_one_argument_per_flag(self):
        """The real pkgconf renders the .pc of a spaced prefix; each flag must survive as one argument."""
        vcpkg = os.environ.get('VCPKG_ROOT', '')
        candidates = [Path(vcpkg) / 'installed/x64-windows/tools/pkgconf/pkgconf.exe'] if vcpkg else []
        candidates += [Path(found) for name in ('pkgconf', 'pkg-config') if (found := shutil.which(name))]
        pkgconf = next((path for path in candidates if path.is_file()), None)
        if pkgconf is None:
            self.skipTest('no pkgconf or pkg-config on this host')
        with tempfile.TemporaryDirectory() as directory:
            prefix = Path(directory).resolve() / 'chosen prefix'
            pkgconfig = prefix / 'lib' / 'pkgconfig'
            pkgconfig.mkdir(parents=True)
            (pkgconfig / 'tkrzw.pc').write_text(windows.pc_text(prefix, PIN['version']), encoding='utf-8')
            env = {**os.environ, 'PKG_CONFIG_PATH': str(pkgconfig), 'PKG_CONFIG_LIBDIR': str(pkgconfig)}
            done = subprocess.run([str(pkgconf), '--cflags', '--libs', 'tkrzw'], env=env,
                                  capture_output=True, text=True, check=True)
            # pkgconf escapes the space; a consumer that honours the escape gets whole paths.
            self.assertEqual(shlex.split(done.stdout),
                             [f'-I{prefix.as_posix()}/include', f'-L{prefix.as_posix()}/lib', '-ltkrzw'])
            self.assertEqual(subprocess.run([str(pkgconf), '--modversion', 'tkrzw'], env=env,
                                            capture_output=True, text=True, check=True).stdout.strip(), PIN['version'])


class CommandTests(unittest.TestCase):
    def test_default_prefix_is_the_cache_root_entry_for_the_revision(self):
        self.assertEqual(windows.default_prefix({'OXPINYIN_CACHE': str(Path.cwd())}, PIN['revision']),
                         Path.cwd() / f'tkrzw-{PIN["revision"][:12]}')
        with self.assertRaises(SystemExit):
            windows.default_prefix({}, PIN['revision'])

    def test_environment_puts_the_prefix_first_on_pkg_config_path(self):
        child = windows.environment(Path('/p'), {'PKG_CONFIG_PATH': '/vcpkg/lib/pkgconfig', 'OTHER': '1'})
        self.assertEqual(child['PKG_CONFIG_PATH'], str(Path('/p/lib/pkgconfig')) + os.pathsep + '/vcpkg/lib/pkgconfig')
        self.assertEqual(child['OTHER'], '1')
        self.assertEqual(windows.environment(Path('/p'), {})['PKG_CONFIG_PATH'], str(Path('/p/lib/pkgconfig')))

    def test_main_publishes_the_pkg_config_path_and_reports_a_refusal(self):
        with tempfile.TemporaryDirectory() as directory:
            prefix = Path(directory) / 'prefix'
            github_env = Path(directory) / 'env'
            with mock.patch.object(windows, 'build', return_value=prefix) as build, \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                status = windows.main(['--prefix', str(prefix), '--github-env'],
                                      {'GITHUB_ENV': str(github_env), 'PKG_CONFIG_PATH': 'vcpkg'})
            self.assertEqual(status, 0)
            self.assertEqual(build.call_args.args[0], prefix)
            expected = str(prefix / 'lib' / 'pkgconfig') + os.pathsep + 'vcpkg'
            self.assertEqual(github_env.read_text(), f'PKG_CONFIG_PATH={expected}\n')
            self.assertIn(f'set PKG_CONFIG_PATH={expected}', output.getvalue())
            with mock.patch.object(windows, 'build', side_effect=windows.RecipeError('nmake exited with status 2')), \
                    contextlib.redirect_stderr(io.StringIO()) as error:
                self.assertEqual(windows.main(['--prefix', str(prefix)], {}), 1)
            self.assertEqual(error.getvalue(), 'FAIL: nmake exited with status 2\n')


if __name__ == '__main__':
    unittest.main()
