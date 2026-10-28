#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for pinyin return and output-parameter cases."""
import ctypes as C
import unittest
from unittest.mock import Mock

import contract_pinyin_out_parameters as family


def declarations(rest_type):
    result = {}

    def case(name, **metadata):
        def collect(fn):
            result[name] = (metadata, fn)
            return fn
        return collect

    family.register(case, Rest=rest_type)
    return result


class PinyinOutParametersTests(unittest.TestCase):
    def test_key_rest_case_uses_the_injected_layout_and_preserves_each_pair(self):
        class Rest(C.Structure):
            _fields_ = [('begin', C.c_ushort), ('end', C.c_ushort)]

        observed = []

        def get_length(inst, rest, out):
            self.assertIsInstance(rest._obj, Rest)
            observed.append((rest._obj.begin, rest._obj.end))
            C.cast(out, C.POINTER(C.c_ushort))[0] = 7
            return True

        kit = Mock()
        kit.fn.return_value = get_length
        result = declarations(Rest)['key-rest-length'][1](kit)
        self.assertEqual(observed, [(0, 2), (5, 2), (2, 2), (0, 65535), (65535, 0), (1, 65535)])
        self.assertEqual(result['5..2'], [True, 7])
        self.assertEqual(result['65535..0'], [True, 7])
        kit.fn.assert_called_once_with('get_pinyin_key_rest_length', family.B, family.P,
                                       C.POINTER(Rest), C.POINTER(C.c_ushort))

    def test_repeated_unload_attempts_keep_the_original_order(self):
        kit = Mock()
        unload = Mock(side_effect=[True, False, False, True, True, False, False])
        kit.fn.return_value = unload
        run = declarations(object())['unload-phrase-library-repeat'][1]
        self.assertEqual(run(kit)['unload'], [True, False, False, True, True, False, False])
        self.assertEqual([call.args[1] for call in unload.call_args_list], [2, 2, 2, 1, 3, 15, 2])


if __name__ == '__main__':
    unittest.main()
