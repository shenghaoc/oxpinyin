#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for scheme and library refusal registration."""
from pathlib import Path
import sys

import ctypes as C
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_scheme_library_refusals as family


def declarations(register):
    return {name: (metadata, fn) for name, _, metadata, fn in collect_declarations(register)}


class SchemeLibraryRefusalsTests(unittest.TestCase):
    def test_both_facades_use_the_shared_context_call_builder(self):
        builder = Mock(side_effect=lambda name, result, *args: lambda k: {'export': name, 'args': args})
        with patch.object(family, 'ctx_call', builder):
            pinyin = declarations(family.register_pinyin)
            zhuyin = declarations(family.register_zhuyin)
        self.assertEqual(builder.call_count, 20)
        self.assertIn(('set_full_pinyin_scheme', family.B, (family.I, 0)),
                      [call.args for call in builder.call_args_list])
        self.assertIn(('unload_phrase_library', family.B, (C.c_ubyte, 255)),
                      [call.args for call in builder.call_args_list])
        self.assertEqual(pinyin['abort-load-phrase-library-8'][1](None)['export'], 'load_phrase_library')
        self.assertEqual(zhuyin['abort-zhuyin-chewing-scheme-minus-1'][0],
                         {'mode': 'zhuyin', 'abort': False, 'issues': ('525',)})

    def test_bigram_iterator_case_reports_each_untouched_output(self):
        kit, calls = Mock(), {}
        kit.fn.side_effect = lambda name, *types: calls.setdefault(name, Mock(return_value=False))
        run = declarations(family.register_pinyin)['abort-bigram-iterator-get-next-phrase'][1]
        self.assertEqual(run(kit), {'has next': False, 'ret': False, 'untouched phrase': True,
                                    'untouched pinyin': True, 'untouched count': True})
        calls['bigram_iterator_get_next_phrase'].assert_called_once()


if __name__ == '__main__':
    unittest.main()
