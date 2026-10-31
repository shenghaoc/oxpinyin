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
from tools.dev.abi import HostFacts, exported_symbols, header_slots, register_shape, version_globals


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

    def test_register_shape_and_both_negative_controls(self):
        self.assertEqual(register_shape('# comment\nname out g_free note pin-probe:valid\n'), [])
        self.assertIn('4 fields, not 5', register_shape('name out g_free note')[0])
        self.assertIn('not a documented kind', register_shape('name out g_free note unverifed')[0])

    def test_linux_dynamic_symbols(self):
        symbols = '10 T pinyin_init\n20 W hook\n30 D private\n40 t local\n'
        self.assertEqual(exported_symbols(symbols, HostFacts('Linux')), {'pinyin_init', 'hook', 'local'})

    def test_darwin_symbols(self):
        symbols = '10 T _pinyin_init\n20 W _hook\n30 T __other\n'
        self.assertEqual(exported_symbols(symbols, HostFacts('Darwin')), {'pinyin_init', '_other'})

    def test_version_scope_excludes_local(self):
        self.assertEqual(version_globals('API {\n global:\n pinyin_init;\n pinyin_init;\n local:\n *;\n};\n'),
                         {'pinyin_init'})



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
        self.assertEqual([argv[:4] for argv in probes], [
            ['libpinyin_capi.so', '/pin/lib/libpinyin/data', 'pinyin', 'subject'],
            ['/pin/lib/libpinyin.so', '/pin/lib/libpinyin/data', 'pinyin', 'pin'],
            ['libzhuyin_capi.so', '/pin/lib/libpinyin/data', 'zhuyin', 'subject'],
            ['/pin/lib/libzhuyin.so', '/pin/lib/libpinyin/data', 'zhuyin', 'pin']])
        self.assertEqual([len(argv) for argv in probes], [5, 5, 5, 5])
        self.assertEqual([Path(probes[0][4]).name, probes[1][4], Path(probes[2][4]).name, probes[3][4]],
                         ['user-pinyin', '', 'user-zhuyin', ''])
        self.assertIn('pinyin injection-disabled negative control: rejected', output)
        self.assertIn('zhuyin injection-disabled negative control: rejected', output)

    def test_the_control_is_the_same_probe_built_without_injection(self):
        _, calls, _, _ = self.gate()
        compiles = [argv for argv in calls if argv[0] == 'g++']
        self.assertEqual([Path(argv[argv.index('-o') + 1]).name for argv in compiles], ['probe', 'control'])
        self.assertEqual([('-DOXPINYIN_ALLOC_FAILURE_DISABLED' in argv) for argv in compiles], [False, True])
        self.assertTrue(all('tools/abi/alloc-failure.cc' in argv for argv in compiles))

    def test_a_failed_probe_fails_the_gate(self):
        status, _, _, _ = self.gate(probe_status=1)
        self.assertEqual(status, 1)

    def test_a_control_that_passes_fails_the_gate(self):
        status, _, _, errors = self.gate(control_status=0)
        self.assertEqual(status, 1)
        self.assertIn('failure-disabled control unexpectedly passed: pinyin', errors)


if __name__ == '__main__':
    unittest.main()
