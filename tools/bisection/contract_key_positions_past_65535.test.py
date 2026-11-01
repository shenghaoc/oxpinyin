#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the #1501 key-position cases past byte 65535."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_key_positions_past_65535 as family


def declarations():
    return {name: (args, kwargs, fn) for name, args, kwargs, fn in collect_declarations(family.register)}


class KeyPositionsPast65535Tests(unittest.TestCase):
    def test_registration_order_and_metadata(self):
        first, second = declarations(), declarations()
        record = '1501-guint16-key-positions.md'
        expected = []
        for label, mode in (('double-pinyin', 'pinyin'), ('chewing-pinyin', 'pinyin'),
                            ('chewing-zhuyin', 'zhuyin')):
            prefix = 'key-position-past-65535/' + label
            expected += [(prefix + '-crossing-key', {'mode': mode, 'issues': ('1501',)}),
                         (prefix + '-past-key', {'mode': mode, 'issues': ('1501',), 'policy_record': record}),
                         (prefix + '-cursor', {'mode': mode, 'issues': ('1501',), 'policy_record': record})]
        self.assertEqual(list(first), [name for name, _ in expected])
        for name, kwargs in expected:
            self.assertEqual(first[name][:2], ((), kwargs))
            self.assertIs(first[name][2], second[name][2])
            self.assertEqual(first[name][2].__module__, family.__name__)

    def test_the_input_puts_one_key_across_byte_65536_and_one_wholly_past_it(self):
        for label, (_mode, _setter, _scheme, _parser, head, unit) in family.PARSERS.items():
            with self.subTest(label=label):
                text = head + unit * family.UNITS
                self.assertEqual(len(text), family.LENGTH)
                begins = [0] + list(range(len(head), len(text), len(unit)))
                self.assertIn(family.CROSSING, begins)
                self.assertGreater(family.CROSSING + len(unit), 65536)
                self.assertEqual(family.PAST, family.CROSSING + len(unit))
                self.assertEqual(family.LAST, begins[-1])
                self.assertNotIn(family.CURSOR, begins)

    def test_parse_configures_the_scheme_and_parses_on_the_kit_instance(self):
        for label, (mode, setter, scheme, parser, head, unit) in family.PARSERS.items():
            with self.subTest(label=label):
                kit = Mock()
                calls = {'set_options': Mock(return_value=True), setter: Mock(return_value=True),
                         parser: Mock(return_value=family.LENGTH),
                         'get_parsed_input_length': Mock(return_value=family.LENGTH)}
                kit.fn.side_effect = lambda name, *types: calls[name]
                self.assertEqual(family.parsed(kit, label), [family.LENGTH, family.LENGTH])
                calls['set_options'].assert_called_once_with(kit.ctx, 1 << 5)
                calls[setter].assert_called_once_with(kit.ctx, scheme)
                calls[parser].assert_called_once_with(kit.inst, head + unit * family.UNITS)

    def test_a_refused_rest_lookup_reads_nothing_further(self):
        kit = Mock(mode='zhuyin')
        rest = Mock(return_value=False)
        kit.fn.side_effect = lambda name, *types: {'get_zhuyin_key_rest': rest}[name]
        self.assertEqual(family.rest_row(kit, family.PAST), [False])
        self.assertEqual(rest.call_args.args[:2], (kit.inst, family.PAST))


if __name__ == '__main__':
    unittest.main()
