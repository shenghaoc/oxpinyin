#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""ABI parsers use injected host facts on every native platform."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.abi import (HostFacts, allocation_ranges, exported_symbols, header_slots,
                           register_shape, version_globals)
from tools.dev.cell import CellError


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

    def test_injection_requires_unique_adapter_ranges(self):
        symbols = ('10 20 t _RNvExample3ffi10owned_cstr\n'
                   '30 40 t _RINvNtExample3ffi15owned_cstr_listExtra\n'
                   '50 60 t _RINvNtExample3ffi16malloc_cstr_withExtra\n')
        self.assertEqual(allocation_ranges(symbols), ('10', '20', '30', '40', '50:60'))
        with self.assertRaisesRegex(CellError, 'unique adapter'):
            allocation_ranges(symbols + symbols)
        with self.assertRaisesRegex(CellError, 'byte-buffer'):
            allocation_ranges('\n'.join(symbols.splitlines()[:2]))


if __name__ == '__main__':
    unittest.main()
