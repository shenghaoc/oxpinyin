#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Unit tests for the shared assertions that never print a call's arguments."""
from pathlib import Path
import os
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import test_support

SENTINEL = 'environment-sentinel-7d1e'


class SentinelTests(unittest.TestCase):
    """Each helper fails on a launcher call that carries the environment, and prints none of it."""

    def setUp(self):
        self.enterContext(mock.patch.dict(os.environ, OXPINYIN_TEST_SENTINEL=SENTINEL))
        self.launch = mock.Mock()
        self.launch(['child', '--flag'], env=dict(os.environ))
        self.env = self.launch.call_args.kwargs['env']
        self.assertEqual(self.env.get('OXPINYIN_TEST_SENTINEL'), SENTINEL)

    def failure(self, helper, *args):
        """Return the message of the AssertionError that helper(self, *args) raises."""
        with self.assertRaises(AssertionError) as raised:
            helper(self, *args)
        return str(raised.exception)

    # Not assertNotIn(SENTINEL, message): a failure would print the message, and with it the environment.

    def test_assert_not_called(self):
        message = self.failure(test_support.assert_not_called, self.launch)
        self.assertFalse(SENTINEL in message, 'the failure message echoes the environment')

    def test_assert_calls(self):
        message = self.failure(test_support.assert_calls, self.launch, [mock.call(['child', '--flag'], env={})])
        self.assertFalse(SENTINEL in message, 'the failure message echoes the environment')

    def test_assert_same_environment(self):
        expected = {name: value for name, value in self.env.items() if name != 'OXPINYIN_TEST_SENTINEL'}
        message = self.failure(test_support.assert_same_environment, self.env, expected)
        self.assertFalse(SENTINEL in message, 'the failure message echoes the environment')


if __name__ == '__main__':
    unittest.main()
