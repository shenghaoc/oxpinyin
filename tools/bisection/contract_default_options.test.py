#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the #532 default option word cases."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock, call, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_default_options as family


def declarations():
    return {name: (args, kwargs, fn) for name, args, kwargs, fn in collect_declarations(family.register)}


class DefaultOptionTests(unittest.TestCase):
    def test_registration_order_metadata_and_replay(self):
        first, second = declarations(), declarations()
        self.assertEqual(list(first), ['default-option-parse', 'default-option-single-key',
                                       'explicit-use-tone-parse', 'explicit-incomplete-parse'])
        self.assertEqual([kwargs for _, kwargs, _ in first.values()],
                         [{'issues': ('532',)}] * 2 + [{'control': True, 'issues': ('532',)}] * 2)
        for name, (args, _, fn) in first.items():
            self.assertEqual(args, ())
            self.assertIs(fn, second[name][2])
            self.assertEqual(fn.__module__, family.__name__)

    def test_default_cases_leave_the_option_word_and_explicit_cases_set_one_bit(self):
        cases = declarations()
        with patch.object(family, 'consumed', side_effect=lambda k, name, text: (name, text)), \
                patch.object(family, 'key_out', side_effect=lambda k, text: text.upper()):
            kit = Mock()
            self.assertEqual(cases['default-option-parse'][2](kit)['su3cl3'], ('parse_more_full_pinyins', b'su3cl3'))
            self.assertEqual(cases['default-option-single-key'][2](kit)['zhang'], b'ZHANG')
            kit.fn.assert_not_called()
            for name, bit in (('explicit-use-tone-parse', 1 << 5), ('explicit-incomplete-parse', 1 << 3)):
                with self.subTest(name=name):
                    kit = Mock()
                    result = cases[name][2](kit)
                    self.assertEqual(list(result), ['su3cl3', 'ni2hao', 'n', 'zzz', 'nihao'])
                    self.assertEqual(kit.fn.call_args, call('set_options', family.B, family.P, family.U))
                    kit.fn.return_value.assert_called_once_with(kit.ctx, bit)


if __name__ == '__main__':
    unittest.main()
