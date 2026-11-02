#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The missing-DBM fixture must preserve the shared system installation."""
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock

import contract_missing_system_dbm as family


class MissingSystemDbmTests(unittest.TestCase):
    def test_missing_link_is_private_and_fini_runs(self):
        with tempfile.TemporaryDirectory() as root:
            system = Path(root, 'system')
            user = Path(root, 'user')
            system.mkdir()
            user.mkdir()
            for filename in family.TABLES.values():
                (system / filename).write_bytes(b'original')
            (system / 'table.conf').write_text(
                'binary format version:7\nmodel data version:14\ndatabase format:BerkeleyDB\n')
            fini = Mock()
            kit = SimpleNamespace(data=str(system), user=str(user), scratch=root,
                                  init=lambda: 17, _ctx=None, _inst=None,
                                  fn=lambda *args: fini)
            self.assertEqual(family.missing_case('bigram', False)(kit),
                             {'init': True, 'fini': True})
            self.assertEqual((system / 'bigram.db').read_bytes(), b'original')
            self.assertFalse(Path(kit.data, 'bigram.db').exists())
            self.assertTrue(Path(kit.data, 'pinyin_index.bin').is_symlink())
            self.assertIsNone(kit._ctx)
            fini.assert_called_once_with(17)

    def test_failed_init_is_observed_without_dependent_calls(self):
        from unittest.mock import patch
        kit = SimpleNamespace(data='unused', init=lambda: None, _ctx=None, fn=Mock())
        with patch.object(family, 'private_system', return_value='private'), \
             patch.object(family, '_write_user_conf'):
            self.assertEqual(family.missing_case(None, True)(kit), {'init': False})
        kit.fn.assert_not_called()


if __name__ == '__main__':
    unittest.main()
