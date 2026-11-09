#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""ABI parsers use injected host facts on every native platform."""
import contextlib
import io
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev import abi
from tools.dev.abi import HostFacts, darwin_export_trie, exported_symbols, header_slots, version_globals


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

    def test_linux_dynamic_symbols(self):
        symbols = '10 T pinyin_init\n20 W hook\n30 D private\n40 t local\n'
        self.assertEqual(exported_symbols(symbols, HostFacts('Linux')), {'pinyin_init', 'hook', 'local'})

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


class DarwinExportGateTests(unittest.TestCase):
    def gate(self, nm_extra='', trie_extra=''):
        import os
        import tempfile
        with tempfile.TemporaryDirectory() as scratch:
            paths = [Path(scratch) / name for name in ('pinyin.dylib', 'zhuyin.dylib')]
            for path in paths:
                path.touch()
            settings = abi.Settings.read(dict(os.environ, OXPINYIN_CAPI_SO=str(paths[0]),
                                              OXPINYIN_ZHUYIN_SO=str(paths[1])))
            def inspect(argv, **options):
                crate, name = ('oxpinyin-capi', 'libpinyin') if argv[-1] == str(paths[0]) else ('oxpinyin-zhuyin-capi', 'libzhuyin')
                names = version_globals((settings.root / 'crates' / crate / (name + '.ver')).read_text())
                if argv[0] == 'nm':
                    output = ''.join('10 T _' + symbol + '\n' for symbol in names) + nm_extra
                else:
                    output = 'image [arm64]:\n -exports:\n offset symbol\n' + ''.join('0x10 _' + symbol + '\n' for symbol in names) + trie_extra
                return SimpleNamespace(returncode=0, stdout=output, stderr='')
            output = io.StringIO()
            with patch.object(abi.subprocess, 'run', side_effect=inspect), \
                 contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                result = abi.check_exports(settings, ['--shipped'], HostFacts('Darwin'))
            return result, output.getvalue()

    def test_shipped_gate_accepts_exact_installed_sets(self):
        status, output = self.gate()
        self.assertEqual(status, 0, output)
        self.assertIn('PASS:', output)

    def test_shipped_gate_rejects_dependency_data_exports(self):
        status, output = self.gate('20 D _dependency_data\n', '0x20 _dependency_data\n')
        self.assertEqual(status, 1, output)
        self.assertIn('LEAKED non-ABI export: dependency_data', output)

    def test_shipped_gate_rejects_exports_hidden_from_nm(self):
        status, output = self.gate(trie_extra='0x20 _dependency_function\n')
        self.assertEqual(status, 1, output)
        self.assertIn('nm and export trie differ', output)
        self.assertIn('LEAKED non-ABI export: dependency_function', output)


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
                                             ['/pin', 'libpinyin_capi.so', 'libzhuyin_capi.so'])
        return status, calls, output.getvalue(), errors.getvalue()

    def test_each_facade_is_probed_on_both_sides_with_a_user_directory_only(self):
        status, calls, output, _ = self.gate()
        probes = [argv[1:] for argv in calls if Path(argv[0]).name == 'probe']
        self.assertEqual(status, 0)
        data, pin_pinyin, pin_zhuyin = (str(Path(path)) for path in (
            '/pin/lib/libpinyin/data', '/pin/lib/libpinyin.so', '/pin/lib/libzhuyin.so'))
        self.assertEqual([argv[:4] for argv in probes], [
            ['libpinyin_capi.so', data, 'pinyin', 'subject'],
            [pin_pinyin, data, 'pinyin', 'pin'],
            ['libzhuyin_capi.so', data, 'zhuyin', 'subject'],
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
