#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the #587 non-UTF-8 directory name cases.

The directory names are created through patched calls: some file systems
(APFS) refuse a name that is not UTF-8, and the cases run on Linux."""
from pathlib import Path
import sys

import os
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_non_utf8_directories as family


def declarations():
    return {name: (args, kwargs, fn) for name, args, kwargs, fn in collect_declarations(family.register)}


def kit():
    k = Mock(scratch='/scratch', data='/data', user='/user')
    k.init.return_value = None
    return k


class NonUtf8DirectoryTests(unittest.TestCase):
    def test_registration_order_metadata_and_replay(self):
        first, second = declarations(), declarations()
        self.assertEqual(list(first), ['non-utf8-directory-system', 'non-utf8-directory-user',
                                       'non-utf8-directory-both', 'non-utf8-directory-zhuyin-both'])
        self.assertEqual([kwargs for _, kwargs, _ in first.values()],
                         [{'issues': ('587',)}] * 3 + [{'mode': 'zhuyin', 'issues': ('587',)}])
        for name, (args, _, fn) in first.items():
            self.assertEqual(args, ())
            self.assertIs(fn, second[name][2])
            self.assertEqual(fn.__module__, family.__name__)

    def test_each_generated_case_binds_its_own_directory_flags(self):
        cases = declarations()
        with patch.object(family, 'non_utf8_dirs', return_value='observed') as probe:
            for name, flags in (('system', (True, False)), ('user', (False, True)), ('both', (True, True))):
                with self.subTest(name=name):
                    k = kit()
                    self.assertEqual(cases['non-utf8-directory-' + name][2](k), 'observed')
                    probe.assert_called_with(k, *flags)

    def test_only_the_requested_directories_get_an_invalid_byte_and_a_failed_init_stops(self):
        tag = str(os.getpid()).encode()
        for system, user in ((True, False), (False, True), (False, False)):
            with self.subTest(system=system, user=user), patch.object(family.os, 'symlink') as link, \
                    patch.object(family.os, 'mkdir') as made:
                k = kit()
                self.assertEqual(family.non_utf8_dirs(k, system, user), {'init': False})
                bad_system, bad_user = b'/scratch/system-\xff' + tag, b'/scratch/user-\xff' + tag
                k.init.assert_called_once_with(system=bad_system if system else b'/data',
                                               user=bad_user if user else b'/user')
                self.assertEqual(link.call_args_list, [((b'/data', bad_system),)] if system else [])
                self.assertEqual(made.call_args_list, [((bad_user,),)] if user else [])
                k.fn.assert_not_called()


if __name__ == '__main__':
    unittest.main()
