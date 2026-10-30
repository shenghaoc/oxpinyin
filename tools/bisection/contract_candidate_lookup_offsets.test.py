#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the candidate lookup offset families."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_candidate_lookup_offsets as family


def declarations(pinyin, zhuyin):
    return {name: (metadata, fn) for name, _, metadata, fn in collect_declarations(family.register, guess_rows=pinyin, zhuyin_guess_rows=zhuyin)}


class CandidateLookupOffsetsTests(unittest.TestCase):
    def test_reserved_slot_case_passes_offsets_and_sort_to_the_injected_reader(self):
        kit, pinyin, zhuyin = Mock(), Mock(side_effect=lambda k, inst, offset, sort: [offset, sort]), Mock()
        with patch.object(family, 'full_inst', return_value='parsed'):
            run = declarations(pinyin, zhuyin)['guess-candidates-past-the-reserved-slot'][1]
            self.assertEqual(run(kit), {'offset 5': [5, 0], 'offset 6': [6, 0]})
        self.assertEqual([call.args[2:] for call in pinyin.call_args_list], [(5, 0), (6, 0)])
        zhuyin.assert_not_called()

    def test_zhuyin_lookup_visits_the_reserved_slot_in_both_sentence_states(self):
        kit, pinyin, zhuyin = Mock(), Mock(), Mock(return_value='rows')
        kit.fn.side_effect = lambda name, *types: Mock(return_value=1 if name == 'parse_more_chewings' else True)
        metadata, run = declarations(pinyin, zhuyin)['guess-candidates-lookup-offset-zhuyin']
        result = run(kit)
        self.assertEqual(metadata, {'mode': 'zhuyin', 'control': True, 'issues': ('696', 'row:65')})
        self.assertEqual(result['su3 prev=0 off=0'], 'rows')
        self.assertEqual(result['su3 prev=1 off=1'], 'rows')
        self.assertEqual(zhuyin.call_count, 40)
        pinyin.assert_not_called()


if __name__ == '__main__':
    unittest.main()
