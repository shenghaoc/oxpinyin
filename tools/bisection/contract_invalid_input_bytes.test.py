#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the #587 invalid UTF-8 input byte cases."""
import unittest
from unittest.mock import Mock, call, patch

import contract_invalid_input_bytes as family


def declarations():
    cases = {}

    def case(name, *args, **kwargs):
        def collect(fn):
            cases[name] = (args, kwargs, fn)
            return fn
        return collect

    family.register(case)
    return cases


class InvalidInputByteTests(unittest.TestCase):
    def test_registration_order_metadata_and_replay(self):
        first, second = declarations(), declarations()
        self.assertEqual(list(first), ['bytes-parse-more-full', 'bytes-parse-more-double', 'bytes-parse-more-chewing',
                                       'bytes-add-phrase-pinyin', 'bytes-whole-argument', 'bytes-zhuyin-parse-more',
                                       'bytes-zhuyin-add-phrase'])
        self.assertEqual(first['bytes-whole-argument'][1], {'control': True, 'issues': ('587',)})
        self.assertEqual(first['bytes-zhuyin-parse-more'][1], {'mode': 'zhuyin', 'issues': ('587',)})
        self.assertEqual(first['bytes-zhuyin-add-phrase'][1], {'mode': 'zhuyin', 'control': True, 'issues': ('587',)})
        for name, (args, kwargs, fn) in first.items():
            self.assertEqual((args, kwargs['issues']), ((), ('587',)))
            self.assertIs(fn, second[name][2])
            self.assertEqual(fn.__module__, family.__name__)

    def test_each_parser_sees_invalid_bytes_before_inside_and_after_the_input(self):
        cases = declarations()
        with patch.object(family, 'consumed', side_effect=lambda k, name, text: name) as parsed:
            for name, parser in (('bytes-parse-more-full', 'parse_more_full_pinyins'),
                                 ('bytes-parse-more-double', 'parse_more_double_pinyins'),
                                 ('bytes-parse-more-chewing', 'parse_more_chewings')):
                with self.subTest(name=name):
                    result = cases[name][2](Mock())
                    self.assertTrue(all(value == parser for value in result.values()))
                    self.assertTrue(all('\\xff' in key or '\\xe4' in key or '\\xc0' in key for key in result))
            zhuyin = cases['bytes-zhuyin-parse-more'][2](Mock())
        self.assertEqual(len(zhuyin), 6)
        self.assertEqual(parsed.call_count, 5 + 3 + 3 + 6)

    def test_zhuyin_add_phrase_uses_a_fresh_iterator_per_reading(self):
        kit = Mock()
        functions = {name: Mock(name=name) for name in ('begin_add_phrases', 'iterator_add_phrase', 'end_add_phrases')}
        functions['begin_add_phrases'].side_effect = ['first', 'second', 'third']
        kit.fn.side_effect = lambda name, *types: functions[name]
        result = declarations()['bytes-zhuyin-add-phrase'][2](kit)
        self.assertEqual(list(result), ['whole', 'tail-invalid', 'mid-invalid'])
        self.assertEqual(functions['end_add_phrases'].call_args_list, [call('first'), call('second'), call('third')])
        readings = [added.args[2] for added in functions['iterator_add_phrase'].call_args_list]
        self.assertEqual(readings[1], 'ㄋㄧˇ ㄏㄠˇ'.encode() + b'\xff')
        self.assertIn(b'\xff', readings[2][:-1])


if __name__ == '__main__':
    unittest.main()
