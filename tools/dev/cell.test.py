#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Portable cell settings, loader paths and atomic ownership tests."""
import json
from pathlib import Path
import tempfile
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tools.dev.cell import CellError, ROOT, Settings, bash_word, library_path


class SettingsTests(unittest.TestCase):
    def test_invalid_setting_diagnostics_keep_shell_escaping(self):
        self.assertEqual(bash_word('bad value'), r'bad\ value')
        self.assertEqual(bash_word('bad;value'), r'bad\;value')
        self.assertEqual(bash_word('bad\nvalue'), "$'bad\\nvalue'")
        self.assertEqual(bash_word(''), "''")

    def test_cells_are_explicit_and_preserve_overrides(self):
        for cell, feature, extension in [('bdb', 'bdb', 'db'), ('kc', 'kyotocabinet', 'kct'),
                                          ('tkrzw', 'tkrzw', 'tkt')]:
            settings = Settings.read({'PINYIN_ORACLE_DBM': cell, 'CARGO_TARGET_DIR': 'custom',
                                      'OXPINYIN_CAPI_SO': 'built/subject.so'})
            self.assertEqual((settings.cell, settings.feature, settings.extension),
                             (cell, feature, extension))
            self.assertEqual(settings.target, ROOT / 'custom')
            self.assertEqual(settings.environment['OXPINYIN_CAPI_SO'], str(ROOT / 'built/subject.so'))
            self.assertTrue(settings.expected_pin.endswith('+dbm-' + cell))

    def test_unknown_cell_is_status_three(self):
        with self.assertRaises(CellError) as error:
            Settings.read({'PINYIN_ORACLE_DBM': 'typo'})
        self.assertEqual(error.exception.status, 3)

    def test_claim_is_atomic_and_rejects_another_owner(self):
        with tempfile.TemporaryDirectory() as scratch:
            settings = Settings.read({'CARGO_TARGET_DIR': scratch})
            settings.claim()
            settings.claim()
            path = Path(scratch) / 'oxpinyin-cell-identity.json'
            owner = json.loads(path.read_text())
            self.assertEqual(owner['artifacts'], {})
            other = Settings.read({'CARGO_TARGET_DIR': scratch, 'PINYIN_ORACLE_DBM': 'kc'})
            with self.assertRaises(CellError) as error:
                other.claim()
            self.assertEqual(error.exception.status, 3)
            self.assertEqual(json.loads(path.read_text()), owner)
            self.assertEqual(list(Path(scratch).iterdir()), [path])

    def test_unreadable_claim_fails_closed(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / 'oxpinyin-cell-identity.json'
            path.write_text('[]')
            settings = Settings.read({'CARGO_TARGET_DIR': scratch})
            with self.assertRaisesRegex(CellError, 'unreadable identity record'):
                settings.claim()
            self.assertEqual(path.read_text(), '[]')

    def test_empty_selection_does_not_claim(self):
        with tempfile.TemporaryDirectory() as scratch:
            settings = Settings.read({'CARGO_TARGET_DIR': scratch})
            self.assertEqual(settings.build([]), 0)
            self.assertEqual(list(Path(scratch).iterdir()), [])

    def test_loader_entries_keep_tokens_and_caller_spelling(self):
        cwd = Path('/caller root')
        self.assertEqual(library_path('./lib::/absolute:lib:', cwd),
                         '/caller root/./lib:/caller root:/absolute:/caller root/lib:/caller root')
        for token in ('$ORIGIN', '${ORIGIN}', '$LIB', '${LIB}', '$PLATFORM', '${PLATFORM}'):
            value = 'prefix/' + token + '/lib'
            self.assertEqual(library_path(value, cwd), value)
        self.assertEqual(library_path('', cwd), '')


if __name__ == '__main__':
    unittest.main()
