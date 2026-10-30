#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the 256th-reading case registrations."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_reading_256th as family


def declarations():
    return {name: (metadata, fn) for name, _, metadata, fn in collect_declarations(family.register)}


class Reading256thTests(unittest.TestCase):
    def test_each_registered_case_selects_its_facade_readings_and_iterator_mode(self):
        kit = Mock()
        cases = declarations()
        with patch.object(family, '_wrap_pinyin_readings', return_value=[b'p']) as pinyin, \
                patch.object(family, '_wrap_zhuyin_readings', return_value=[b'z']) as zhuyin, \
                patch.object(family, 'wrap_256th_reading', return_value='observed') as wrap:
            for name, readings, same in (('wrap-256th-reading', [b'p'], False),
                                         ('wrap-256th-reading-same-iterator', [b'p'], True),
                                         ('wrap-256th-reading-zhuyin', [b'z'], False),
                                         ('wrap-256th-reading-zhuyin-same-iterator', [b'z'], True)):
                with self.subTest(name=name):
                    self.assertEqual(cases[name][1](kit), 'observed')
                    wrap.assert_called_with(kit, readings, same_iterator=same)
        self.assertEqual((pinyin.call_count, zhuyin.call_count), (2, 2))

    def test_candidate_readings_keep_the_tone_order(self):
        self.assertEqual(family._wrap_pinyin_readings()[:4], [b'ba1', b'ba2', b'ba3', b'ba4'])
        self.assertEqual(family._wrap_zhuyin_readings()[:4],
                         [text.encode() for text in ('ㄅㄚˊ', 'ㄅㄚˇ', 'ㄅㄚˋ', 'ㄅㄚ')])


if __name__ == '__main__':
    unittest.main()
