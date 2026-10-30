#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for combined import and n-best ordering cases."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_combined_import_nbest as family


def declarations(export, files):
    return {name: (metadata, fn) for name, _, metadata, fn in collect_declarations(family.register, remember_export=export, transient_files=files)}


class CombinedImportNbestTests(unittest.TestCase):
    def test_sixth_add_checks_the_injected_export_and_file_snapshots(self):
        kit, calls = Mock(mode='pinyin', user='user'), {}
        export, files = Mock(return_value=['reading']), Mock(return_value={'file': b'data'})
        tokens = [16802309, 33570045, 50359794, 67109866, 83886081]

        def resolve(name, *types):
            if name not in calls:
                if name == 'iterator_add_phrase':
                    calls[name] = Mock(side_effect=[True] * 5 + [False])
                elif name == 'save':
                    calls[name] = Mock(side_effect=[True, False])
                else:
                    calls[name] = Mock(return_value=True)
            return calls[name]

        kit.fn.side_effect = resolve
        with patch.object(family, 'tokens_of', return_value=tokens), \
                patch.object(family, 'unigram_of', return_value=9):
            run = declarations(export, files)['combined-library-sixth-add-pinyin'][1]
            result = run(kit)
        self.assertEqual(result, {'ret': False, 'untouched-tokens': True, 'untouched-frequencies': True,
                                  'untouched-readings': True, 'untouched-dirty': True, 'untouched-files': True})
        self.assertEqual(export.call_count, 14)
        self.assertEqual(files.call_count, 2)
        self.assertEqual([call.args[1] for call in calls['begin_add_phrases'].call_args_list], [1, 2, 3, 4, 5, 6])

    def test_fifth_add_control_does_not_take_export_or_file_snapshots(self):
        kit, export, files = Mock(mode='zhuyin'), Mock(), Mock()
        kit.fn.return_value = Mock(return_value=True)
        result = declarations(export, files)['combined-library-fifth-add-zhuyin'][1](kit)
        self.assertEqual(result, {'added': [True] * 5})
        export.assert_not_called()
        files.assert_not_called()


if __name__ == '__main__':
    unittest.main()
