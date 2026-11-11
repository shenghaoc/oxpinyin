"""The shared tkrzw pin and installed Windows consumer metadata."""
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

    def test_installed_linkage_condition_must_occur_exactly_once(self):
        condition = '#if defined(_WIN32) && defined(TKRZW_SHARED)\n'
        with tempfile.TemporaryDirectory() as directory:
            prefix = Path(directory)
            header = prefix / 'include/tkrzw_lib_common.h'
            header.parent.mkdir()
            script = prefix / 'test.cmake'
            script.write_text(f'''
set(VERSION "{build.PIN['version']}")
set(CURRENT_PACKAGES_DIR "{prefix.as_posix()}")
set(SOURCE_PATH "{prefix.as_posix()}")
set(PORT tkrzw)
function(vcpkg_from_github)
endfunction()
function(vcpkg_check_features)
endfunction()
function(vcpkg_cmake_configure)
endfunction()
function(vcpkg_cmake_install)
endfunction()
function(vcpkg_copy_pdbs)
endfunction()
function(vcpkg_fixup_pkgconfig)
endfunction()
function(vcpkg_install_copyright)
endfunction()
include("{(ROOT / 'vcpkg-overlays/tkrzw/portfile.cmake').as_posix()}")
''')
            for linkage in ('static', 'dynamic'):
                for count in (0, 1, 2):
                    with self.subTest(linkage=linkage, occurrences=count):
                        original = '// fixture\n' + condition * count
                        header.write_text(original)
                        result = subprocess.run(
                            ['cmake', f'-DVCPKG_LIBRARY_LINKAGE={linkage}', '-P', str(script)],
                            capture_output=True, text=True)
                        if count == 1:
                            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                            expected = '#if 0' if linkage == 'static' else '#if defined(_WIN32)'
                            self.assertEqual(header.read_text(), '// fixture\n' + expected + '\n')
                        else:
                            self.assertNotEqual(result.returncode, 0)
                            self.assertIn('Expected exactly one DLL linkage condition', result.stderr)
                            self.assertEqual(header.read_text(), original)

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
