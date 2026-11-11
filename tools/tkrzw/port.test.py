"""The shared tkrzw pin, the port's build route per host and the installed
Windows consumer metadata."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.tkrzw import build


class PortTests(unittest.TestCase):
    def test_port_version_matches_the_shared_pin(self):
        port = ROOT / 'vcpkg-overlays/tkrzw'
        self.assertEqual(json.loads((port / 'vcpkg.json').read_text())['version'], build.PIN['version'])

    def script(self, prefix):
        """A script that runs the portfile over PREFIX with vcpkg's functions
        replaced: each records its name and arguments in PREFIX/calls."""
        stubs = ''.join(f'function({name})\n    file(APPEND "{prefix.as_posix()}/calls" "{name} ${{ARGN}}\\n")\nendfunction()\n'
                        for name in ('vcpkg_from_github', 'vcpkg_check_features', 'vcpkg_cmake_configure',
                                     'vcpkg_cmake_install', 'vcpkg_make_configure', 'vcpkg_make_install',
                                     'vcpkg_copy_pdbs', 'vcpkg_fixup_pkgconfig', 'vcpkg_install_copyright'))
        script = prefix / 'test.cmake'
        script.write_text(f'''
set(VERSION "{build.PIN['version']}")
set(CURRENT_PACKAGES_DIR "{prefix.as_posix()}")
set(SOURCE_PATH "{prefix.as_posix()}")
set(PORT tkrzw)
{stubs}
include("{(ROOT / 'vcpkg-overlays/tkrzw/portfile.cmake').as_posix()}")
''')
        return script

    def test_installed_linkage_condition_must_occur_exactly_once(self):
        condition = '#if defined(_WIN32) && defined(TKRZW_SHARED)\n'
        with tempfile.TemporaryDirectory() as directory:
            prefix = Path(directory)
            header = prefix / 'include/tkrzw_lib_common.h'
            header.parent.mkdir()
            script = self.script(prefix)
            for linkage in ('static', 'dynamic'):
                for count in (0, 1, 2):
                    with self.subTest(linkage=linkage, occurrences=count):
                        original = '// fixture\n' + condition * count
                        header.write_text(original)
                        result = subprocess.run(
                            ['cmake', '-DVCPKG_TARGET_IS_WINDOWS=ON', f'-DVCPKG_LIBRARY_LINKAGE={linkage}', '-P', str(script)],
                            capture_output=True, text=True)
                        if count == 1:
                            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                            expected = '#if 0' if linkage == 'static' else '#if defined(_WIN32)'
                            self.assertEqual(header.read_text(), '// fixture\n' + expected + '\n')
                        else:
                            self.assertNotEqual(result.returncode, 0)
                            self.assertIn('Expected exactly one DLL linkage condition', result.stderr)
                            self.assertEqual(header.read_text(), original)

    def test_macos_runs_upstream_configure_and_fixes_the_installed_files(self):
        for linkage, options, archive in (('dynamic', ';OPTIONS', False), ('static', ';OPTIONS;--disable-shared', True)):
            with self.subTest(linkage=linkage), tempfile.TemporaryDirectory() as directory:
                prefix = Path(directory)
                for name in ('include/tkrzw_lib_common.h', 'lib/libtkrzw.a', 'lib/pkgconfig/tkrzw.pc'):
                    (prefix / name).parent.mkdir(parents=True, exist_ok=True)
                (prefix / 'include/tkrzw_lib_common.h').write_text('#if defined(_WIN32) && defined(TKRZW_SHARED)\n')
                (prefix / 'lib/libtkrzw.a').write_bytes(b'archive')
                (prefix / 'lib/pkgconfig/tkrzw.pc').write_text('Libs: -L${libdir} -ltkrzw -lstdc++ -lpthread\n')
                result = subprocess.run(
                    ['cmake', '-DVCPKG_TARGET_IS_OSX=ON', f'-DVCPKG_LIBRARY_LINKAGE={linkage}', '-P', str(self.script(prefix))],
                    capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                calls = (prefix / 'calls').read_text().splitlines()
                self.assertEqual([call.split()[0] for call in calls],
                                 ['vcpkg_from_github', 'vcpkg_make_configure', 'vcpkg_make_install',
                                  'vcpkg_copy_pdbs', 'vcpkg_fixup_pkgconfig', 'vcpkg_install_copyright'])
                # No AUTORECONF: upstream commits its configure script.
                self.assertEqual(calls[1], f'vcpkg_make_configure SOURCE_PATH;{prefix.as_posix()};COPY_SOURCE;'
                                           f'DEFAULT_OPTIONS_EXCLUDE;^--(enable|disable)-(shared|static)$' + options)
                self.assertEqual((prefix / 'lib/libtkrzw.a').exists(), archive)
                self.assertEqual((prefix / 'lib/pkgconfig/tkrzw.pc').read_text(), 'Libs: -L${libdir} -ltkrzw -lc++ -lpthread\n')

    if sys.platform == 'win32':
        def test_installed_metadata_preserves_spaced_prefix(self):
            installed = Path(os.environ['VCPKG_ROOT']) / 'installed/x64-windows'
            pkgconf = installed / 'tools/pkgconf/pkgconf.exe'
            with tempfile.TemporaryDirectory() as directory:
                prefix = Path(directory) / 'chosen prefix'
                pcdir = prefix / 'lib/pkgconfig'
                pcdir.mkdir(parents=True)
                (pcdir / 'tkrzw.pc').write_bytes((installed / 'lib/pkgconfig/tkrzw.pc').read_bytes())
                result = subprocess.run([str(pkgconf), '--msvc-syntax', '--cflags', '--libs', 'tkrzw'],
                                        env={**os.environ, 'PKG_CONFIG_PATH': str(pcdir),
                                             'PKG_CONFIG_LIBDIR': str(pcdir)},
                                        capture_output=True, text=True, check=True)
                self.assertEqual(result.stdout.strip(),
                                 f'/I"{prefix.as_posix()}/include" '
                                 f'/libpath:"{prefix.as_posix()}/lib" tkrzw.lib')


if __name__ == '__main__':
    unittest.main()
