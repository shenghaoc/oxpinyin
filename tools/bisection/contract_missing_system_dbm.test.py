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

    def test_only_the_bdb_twins_are_registered_and_they_name_the_record(self):
        cases = {}

        def case(name, **spec):
            return lambda fn: cases.setdefault(name, spec)

        family.register(case)
        self.assertEqual(len(cases), 14 + 8)
        for name, spec in cases.items():
            self.assertIn('/bdb-', name)
            self.assertEqual(spec['cells'], ('bdb',), name)
            self.assertEqual(spec['policy_record'], family.RECORD, name)
        for table in ('punctuation', 'addon-pinyin', 'addon-phrase'):
            for operation in ('lifecycle', 'operations', 'overlay'):
                self.assertNotIn(f'missing-system-dbm/bdb-zhuyin-{table}-{operation}', cases)
        self.assertEqual(sorted(name for name in cases if name.endswith('-lifecycle')),
                         ['missing-system-dbm/bdb-pinyin-bigram-lifecycle',
                          'missing-system-dbm/bdb-zhuyin-phrase-lifecycle'])


if __name__ == '__main__':
    unittest.main()
