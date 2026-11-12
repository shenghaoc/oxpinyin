#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""The local cross-open command keeps its cell, data and pin inputs explicit."""
import contextlib
import io
import sys
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import reverse_cross_open


class CommandTests(unittest.TestCase):
    def main(self, argv):
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr), contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as raised:
                reverse_cross_open.main(argv)
        return raised.exception.code, stderr.getvalue()

    def test_the_tkrzw_cell_requires_the_package_data(self):
        code, stderr = self.main(['--cell', 'tkrzw', '/subject.so'])
        self.assertEqual(code, 2)
        self.assertIn('--data', stderr)

    def test_the_handoff_check_requires_a_pin_prefix(self):
        code, stderr = self.main(['--cell', 'bdb', '/subject.so'])
        self.assertEqual(code, 2)
        self.assertIn('--pin-prefix', stderr)

    def test_the_handoff_check_requires_both_subject_libraries(self):
        code, stderr = self.main(['--cell', 'bdb', '--pin-prefix', '/pin', '/pinyin.so'])
        self.assertEqual(code, 2)
        self.assertIn('--zhuyin-so', stderr)


class CellScopeTests(unittest.TestCase):
    def test_issue1114_runs_only_on_tkrzw(self):
        self.assertEqual(
            reverse_cross_open.handoff_cases('tkrzw'),
            ['issue1114-collision-load-save-pinyin', 'issue1114-collision-load-save-zhuyin',
             'save-929-logical-cross-open-pinyin', 'save-929-logical-cross-open-zhuyin'])
        self.assertEqual(
            reverse_cross_open.handoff_cases('bdb'),
            ['save-929-logical-cross-open-pinyin', 'save-929-logical-cross-open-zhuyin'])


class SavedBytesTests(unittest.TestCase):
    def test_only_the_timestamp_bytes_are_masked(self):
        raw = bytes(range(128))
        masked = reverse_cross_open.tkrzw_saved_bigram_bytes(raw)
        self.assertEqual(masked[:48], raw[:48])
        self.assertEqual(masked[48:56], b'\0' * 8)
        self.assertEqual(masked[56:], raw[56:])


if __name__ == '__main__':
    unittest.main()
