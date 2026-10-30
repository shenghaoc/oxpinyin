#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for predicted candidate selection and its shared reader."""
from pathlib import Path
import sys

import ctypes as C
import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_predicted_types as family


def declarations(reader):
    return {name: (metadata, fn) for name, _, metadata, fn in collect_declarations(family.register, predicted_rows=reader)}


class PredictedTypesTests(unittest.TestCase):
    def test_selection_uses_the_reader_and_selects_the_requested_candidate_types(self):
        kit = Mock()
        reader = Mock(return_value=([[8, 'punctuation'], [5, 'prefix']], 2))
        chosen = Mock(side_effect=lambda inst, candidate: candidate.value)

        def count(inst, out):
            C.cast(out, C.POINTER(family.U))[0] = 2

        def candidate(inst, index, out):
            C.cast(out, C.POINTER(family.P))[0] = index + 1

        def kind(inst, row, out):
            C.cast(out, C.POINTER(family.I))[0] = (8, 5)[row.value - 1]

        calls = {'get_n_candidate': count, 'get_candidate': candidate,
                 'get_candidate_type': kind, 'choose_predicted_candidate': chosen}
        kit.fn.side_effect = lambda name, *types: calls[name]
        metadata, run = declarations(reader)['choose-predicted-types']
        self.assertEqual(metadata, {'issues': ('542', '540')})
        self.assertEqual(run(kit), {'before': ([[8, 'punctuation'], [5, 'prefix']], 2),
                                    'punctuation': 1, 'punctuation again': 1, 'prefix': 2})
        reader.assert_called_once_with(kit, kit.inst, '我')
        self.assertEqual([call.args[1].value for call in chosen.call_args_list], [1, 1, 2])

    def test_registrations_keep_their_own_reader(self):
        one, two = Mock(return_value='one'), Mock(return_value='two')
        first = declarations(one)['choose-predicted-types'][1]
        second = declarations(two)['choose-predicted-types'][1]
        kit = Mock()
        kit.fn.return_value = Mock()  # the default count is zero
        self.assertEqual(first(kit)['before'], 'one')
        self.assertEqual(second(kit)['before'], 'two')
        one.assert_called_once()
        two.assert_called_once()


if __name__ == '__main__':
    unittest.main()
