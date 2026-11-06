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

    def test_bdb_twins_name_the_record_and_the_others_do_not(self):
        cases = {}

        def case(name, **spec):
            return lambda fn: cases.setdefault(name, spec)

        family.register(case)
        twins = {name: spec for name, spec in cases.items() if '/bdb-' in name}
        self.assertTrue(twins)
        for name, spec in twins.items():
            self.assertEqual(spec['cells'], ('bdb',), name)
            self.assertEqual(spec['policy_record'], family.RECORD, name)
            plain = name.replace('/bdb-', '/')
            self.assertEqual(cases[plain]['cells'], ('kc', 'tkrzw'), plain)
            self.assertNotIn('policy_record', cases[plain])
        self.assertEqual(len(twins), 14 + 8)
        unattached = cases['missing-system-dbm/zhuyin-punctuation-overlay']
        self.assertNotIn('cells', unattached)
        self.assertNotIn('policy_record', unattached)
        for table in ('punctuation', 'addon-pinyin', 'addon-phrase'):
            for operation in ('lifecycle', 'operations'):
                self.assertNotIn(f'missing-system-dbm/zhuyin-{table}-{operation}', cases)
        self.assertEqual(sorted(name for name in cases if name.endswith('-lifecycle')),
                         ['missing-system-dbm/bdb-pinyin-bigram-lifecycle',
                          'missing-system-dbm/bdb-zhuyin-phrase-lifecycle',
                          'missing-system-dbm/pinyin-bigram-lifecycle',
                          'missing-system-dbm/zhuyin-phrase-lifecycle'])
        for facade in ('pinyin', 'zhuyin'):
            intact = cases[f'missing-system-dbm/{facade}-intact']
            self.assertNotIn('cells', intact)
            self.assertTrue(intact['control'])


if __name__ == '__main__':
    unittest.main()
