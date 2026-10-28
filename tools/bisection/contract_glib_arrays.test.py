#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for GLib array registration and null-array observations."""
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import contract_glib_arrays as family


def declarations(register, **kwargs):
    result = {}

    def case(name, **metadata):
        def collect(fn):
            result[name] = (metadata, fn)
            return fn
        return collect

    register(case, **kwargs)
    return result


class GLibArraysTests(unittest.TestCase):
    def test_oom_cases_register_the_exact_shared_driver_helper(self):
        helper = Mock(return_value={'injection': True})
        cases = declarations(family.register_oom, glib_924_oom_array=helper)
        for mode in ('pinyin', 'zhuyin'):
            metadata, fn = cases['glib-924-oom-array-growth-' + mode]
            self.assertIs(fn, helper)
            self.assertEqual(metadata, {'mode': mode, 'issues': ('924', '924-oom')})

    def test_null_array_cases_capture_both_presence_paths_and_remove_the_handler(self):
        for operation in ('lookup', 'pronunciation'):
            with self.subTest(operation=operation):
                install, remove, calls = Mock(return_value=17), Mock(), {}
                glib = SimpleNamespace(g_log_set_handler=install, g_log_remove_handler=remove)
                kit = SimpleNamespace(inst='inst', glib=glib, _handler=lambda *args: None,
                                      logs=[], log_messages=[])
                kit.fn = lambda name, *types: calls.setdefault(name, Mock(side_effect=[True, False]))
                with patch.object(family, 'tokens_of', return_value=[0x01000001]):
                    run = declarations(family.register_null)['glib-924-null-array-' + operation + '-pinyin'][1]
                    result = run(kit)
                self.assertEqual(result['found'], {'ret': True, 'logs': [], 'messages': []})
                self.assertEqual(result['missing'], {'ret': False, 'logs': [], 'messages': []})
                remove.assert_called_once_with(b'GLib', 17)
                export = 'lookup_tokens' if operation == 'lookup' else 'token_get_nth_pronunciation'
                self.assertEqual(calls[export].call_count, 2)


if __name__ == '__main__':
    unittest.main()
