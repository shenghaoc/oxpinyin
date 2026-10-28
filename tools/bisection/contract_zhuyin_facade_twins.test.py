#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the zhuyin facade twin cases."""
import ctypes as C
import unittest
from unittest.mock import Mock

import contract_zhuyin_facade_twins as family


def declarations(rest_type):
    result = {}

    def case(name, **metadata):
        def collect(fn):
            result[name] = (metadata, fn)
            return fn
        return collect

    family.register(case, Rest=rest_type)
    return result


class ZhuyinFacadeTwinsTests(unittest.TestCase):
    def test_rest_length_uses_the_shared_layout_for_each_boundary(self):
        class Rest(C.Structure):
            _fields_ = [('begin', C.c_ushort), ('end', C.c_ushort)]

        pairs = []

        def length(inst, rest, out):
            self.assertIsInstance(rest._obj, Rest)
            pairs.append((rest._obj.begin, rest._obj.end))
            C.cast(out, C.POINTER(C.c_ushort))[0] = 11
            return True

        kit = Mock()
        kit.fn.return_value = length
        metadata, run = declarations(Rest)['zhuyin-key-rest-length']
        self.assertEqual(metadata, {'mode': 'zhuyin', 'issues': ('542',)})
        result = run(kit)
        self.assertEqual(pairs, [(0, 2), (5, 2), (2, 2), (0, 65535), (65535, 0), (1, 65535)])
        self.assertEqual(result['5..2'], [True, 11])
        kit.fn.assert_called_once_with('get_zhuyin_key_rest_length', family.B, family.P,
                                       C.POINTER(Rest), C.POINTER(C.c_ushort))

    def test_repeated_unload_queries_each_library_and_then_library_two_twice(self):
        kit = Mock()
        unload = Mock(return_value=True)
        kit.fn.return_value = unload
        run = declarations(object())['zhuyin-unload-phrase-library-repeat'][1]
        self.assertEqual(run(kit), {'unload': [True] * 18})
        self.assertEqual([call.args[1] for call in unload.call_args_list], list(range(16)) + [2, 2])


if __name__ == '__main__':
    unittest.main()
