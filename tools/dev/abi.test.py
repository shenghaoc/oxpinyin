"""ABI parsers use injected host facts on every native platform."""
import contextlib
import io
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev import abi
from tools.dev.abi import HostFacts, darwin_export_trie, header_slots
from tools.dev.packaging import crate_dir, version_exports
from tools.dev.test_support import fixture_metadata


class SymbolTests(unittest.TestCase):
    def test_pointer_slots_ignore_disabled_declarations(self):
        header = '''// ignored
        #if 0
        void hidden(char **out);
        #endif
        /* comment */ handle_t *new_handle(void);
        bool get_name(const char **name, gchar **owned, gchar ***list, thing **borrowed);
        '''
        self.assertEqual(header_slots(header), [('get_name', 'borrowed', 'opaque-pp'),
                                               ('get_name', 'list', 'strv'),
                                               ('get_name', 'name', 'const-pp'),
                                               ('get_name', 'owned', 'char-pp'),
                                               ('new_handle', 'return', 'ptr-return')])

    def test_darwin_export_trie_includes_data_weak_and_reexports(self):
        text = '''library [arm64]:
    -exports:
        offset      symbol
        0x00000488  _pinyin_init
        0x00004000  _data [weak_def]
        [re-export] _forwarded (from library)
'''
        self.assertEqual(darwin_export_trie(text), {'pinyin_init', 'data', 'forwarded'})
        with self.assertRaisesRegex(ValueError, 'unrecognized'):
            darwin_export_trie('mis-aligned LINKEDIT string pool')


class LinuxExportGateTests(unittest.TestCase):
    def gate(self, extra=''):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for crate, name in [('capi', 'pinyin'), ('zhuyin-capi', 'zhuyin')]:
                (root / crate).mkdir()
                (root / crate / f'lib{name}.ver').write_text(
                    f'API_VERSION {{\n global:\n  {name}_init;\n local:\n  *;\n}};\n')
            settings = SimpleNamespace(root=root, environment={},
                                       artifact=lambda variable, filename, crate: root / filename)

            def inspect(argv, **options):
                self.assertEqual(argv[:3], ['nm', '-D', '--defined-only'])
                name = 'pinyin' if 'pinyin' in Path(argv[-1]).name else 'zhuyin'
                text = f'00 A API_VERSION\n10 T {name}_init@@API_VERSION\n' + extra
                return subprocess.CompletedProcess(argv, 0, text, '')

            output = io.StringIO()
            with patch.object(abi.subprocess, 'run', side_effect=inspect), contextlib.redirect_stdout(output):
                status = abi.check_exports(settings, HostFacts('Linux'))
            return status, output.getvalue()

    def test_declared_version_node_passes(self):
        status, output = self.gate()
        self.assertEqual(status, 0, output)

    def test_each_extra_definition_fails(self):
        for kind, name in [('D', 'private'), ('B', 'buffer'), ('R', 'constant'), ('V', 'weak_object'),
                           ('A', 'API_VERSION_EXTRA'), ('D', 'API_VERSION'), ('A', 'API_VERSION@@OTHER')]:
            with self.subTest(kind=kind, name=name):
                status, output = self.gate(f'30 {kind} {name}\n')
                self.assertEqual(status, 1, output)
                self.assertIn('LEAKED non-ABI export: ' + name.split('@')[0] + '\n', output)


class DarwinExportGateTests(unittest.TestCase):
    def gate(self, nm_extra='', trie_extra='', overrides=True):
        import os
        with tempfile.TemporaryDirectory() as scratch:
            paths = [Path(scratch) / name for name in ('pinyin.dylib', 'zhuyin.dylib')]
            environment = {key: value for key, value in os.environ.items()
                           if key not in ('OXPINYIN_CAPI_SO', 'OXPINYIN_ZHUYIN_SO')}
            environment['PINYIN_ORACLE_DBM'] = 'tkrzw'
            if overrides:
                for path in paths:
                    path.touch()
                environment.update(OXPINYIN_CAPI_SO=str(paths[0]), OXPINYIN_ZHUYIN_SO=str(paths[1]))
            settings = abi.Settings.read(environment)
            self.installs = []

            def staged(arguments, tools):
                self.installs.append(arguments)
                self.assertEqual(tools.env.get('PINYIN_ORACLE_DBM'), 'tkrzw')
                lib = Path(arguments[2].removeprefix('--destdir=')) / 'usr/local/lib'
                lib.mkdir(parents=True, exist_ok=True)
                (lib / (arguments[0] + '.15.0.0.dylib')).touch()
                return 0

            def inspect(argv, **options):
                name = 'libzhuyin' if 'zhuyin' in Path(argv[-1]).name else 'libpinyin'
                _, names = version_exports(crate_dir(settings.root, name.removeprefix('lib')) / (name + '.ver'))
                if argv[0] == 'nm':
                    output = ''.join('10 T _' + symbol + '\n' for symbol in names) + nm_extra
                else:
                    output = 'image [arm64]:\n -exports:\n offset symbol\n' + ''.join('0x10 _' + symbol + '\n' for symbol in names) + trie_extra
                return SimpleNamespace(returncode=0, stdout=output, stderr='')
            output = io.StringIO()
            with fixture_metadata(), patch.object(abi.subprocess, 'run', side_effect=inspect), \
                 patch.object(abi, 'install', side_effect=staged), \
                 contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                result = abi.check_exports(settings, HostFacts('Darwin'))
            return result, output.getvalue()

    def test_installed_gate_accepts_exact_installed_sets(self):
        status, output = self.gate()
        self.assertEqual(status, 0, output)
        self.assertIn('PASS:', output)

    def test_installed_gate_rejects_dependency_data_exports(self):
        status, output = self.gate('20 D _dependency_data\n', '0x20 _dependency_data\n')
        self.assertEqual(status, 1, output)
        self.assertIn('LEAKED non-ABI export: dependency_data', output)

    def test_installed_gate_rejects_exports_hidden_from_nm(self):
        status, output = self.gate(trie_extra='0x20 _dependency_function\n')
        self.assertEqual(status, 1, output)
        self.assertIn('nm and export trie differ', output)
        self.assertIn('LEAKED non-ABI export: dependency_function', output)

    def test_overrides_check_the_given_files_without_staging(self):
        status, output = self.gate()
        self.assertEqual(status, 0, output)
        self.assertEqual(self.installs, [])
        self.assertIn('pinyin.dylib against capi/libpinyin.ver', output)

    def test_without_overrides_the_gate_stages_and_checks_the_shipped_relink(self):
        status, output = self.gate(overrides=False)
        self.assertEqual(status, 0, output)
        self.assertIn('PASS:', output)
        self.assertEqual([arguments[0] for arguments in self.installs], ['libpinyin', 'libzhuyin'])
        for arguments in self.installs:
            self.assertEqual(arguments[1], '--prefix=/usr/local')
            self.assertTrue(arguments[2].startswith('--destdir='))
            self.assertEqual(arguments[3:], ['--', '--release', '--locked', '--no-default-features', '--features', 'tkrzw'])
        stage = self.installs[0][2].removeprefix('--destdir=')
        self.assertEqual(self.installs[1][2].removeprefix('--destdir='), stage)
        self.assertIn(f'{stage}/usr/local/lib/libpinyin.15.0.0.dylib against capi/libpinyin.ver', output)
        self.assertIn(f'{stage}/usr/local/lib/libzhuyin.15.0.0.dylib against zhuyin-capi/libzhuyin.ver', output)
        self.assertFalse(Path(stage).exists(), 'the stage is temporary')

    def test_a_failed_staging_fails_the_gate_with_its_status(self):
        with patch.object(abi, 'install', side_effect=abi.Finished(7)), \
             self.assertRaises(abi.CellError) as raised:
            abi.shipped_library(abi.Settings.read({'PINYIN_ORACLE_DBM': 'tkrzw'}), 'libpinyin', Path('/stage'))
        self.assertEqual(raised.exception.status, 7)
        self.assertIn('staging libpinyin', str(raised.exception))


class AllocationFailureGateTests(unittest.TestCase):
    """The probe fails GLib's allocations by caller; the gate gives it no symbol data (#1227, #1308)."""

    def gate(self, probe_status=0, control_status=1):
        calls = []

        def command(argv, **options):
            calls.append(list(map(str, argv)))
            name = Path(argv[0]).name
            return subprocess.CompletedProcess(argv, control_status if name == 'control' else
                                               probe_status if name == 'probe' else 0)

        output, errors = io.StringIO(), io.StringIO()
        with patch.object(abi, 'driver_services', return_value=SimpleNamespace(command=command)), \
             patch.object(abi.subprocess, 'run', return_value=SimpleNamespace(stdout='-I/glib -lglib-2.0')), \
             contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            status = abi.check_alloc_failure(SimpleNamespace(environment={}),
                                             ['/pin', 'liboxpinyin_capi.so', 'liboxzhuyin_capi.so'])
        return status, calls, output.getvalue(), errors.getvalue()

    def test_each_facade_is_probed_on_both_sides_with_a_user_directory_only(self):
        status, calls, output, _ = self.gate()
        probes = [argv[1:] for argv in calls if Path(argv[0]).name == 'probe']
        self.assertEqual(status, 0)
        data, pin_pinyin, pin_zhuyin = (str(Path(path)) for path in (
            '/pin/lib/libpinyin/data', '/pin/lib/libpinyin.so', '/pin/lib/libzhuyin.so'))
        self.assertEqual([argv[:4] for argv in probes], [
            ['liboxpinyin_capi.so', data, 'pinyin', 'subject'],
            [pin_pinyin, data, 'pinyin', 'pin'],
            ['liboxzhuyin_capi.so', data, 'zhuyin', 'subject'],
            [pin_zhuyin, data, 'zhuyin', 'pin']])
        self.assertEqual([len(argv) for argv in probes], [5, 5, 5, 5])
        self.assertEqual([Path(probes[0][4]).name, probes[1][4], Path(probes[2][4]).name, probes[3][4]],
                         ['user-pinyin', '', 'user-zhuyin', ''])
        self.assertIn('pinyin injection-disabled negative control: rejected', output)
        self.assertIn('zhuyin injection-disabled negative control: rejected', output)

    def test_a_failed_probe_fails_the_gate(self):
        status, _, _, _ = self.gate(probe_status=1)
        self.assertEqual(status, 1)

    def test_a_control_that_passes_fails_the_gate(self):
        status, _, _, errors = self.gate(control_status=0)
        self.assertEqual(status, 1)
        self.assertIn('failure-disabled control unexpectedly passed: pinyin', errors)


if __name__ == '__main__':
    unittest.main()
