#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Unit tests for tools.dev.doctor."""
from pathlib import Path
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import doctor


class DoctorTests(unittest.TestCase):
    def test_current_interpreter_identity(self):
        info = doctor.identity()
        self.assertEqual(info['implementation'], 'cpython')
        self.assertEqual(info['version'], '3.15.0')
        self.assertTrue(info['gil_enabled'])
        self.assertFalse(info['gil_disabled'])
        self.assertTrue(info['managed'])

    def test_selection_overrides_are_rejected(self):
        for name in ('UV_PYTHON', 'PYTHONHOME', 'PYTHONPATH',
                     'UV_PYTHON_VERSION', 'UV_PYTHON_IMPLEMENTATION',
                     'UV_PYTHON_VARIANT', 'UV_PYTHON_ARCH'):
            with self.subTest(name=name):
                with self.assertRaisesRegex(doctor.DoctorError, name):
                    doctor.reject_overrides(dict(doctor.os.environ, **{name: '/x'}))

    def test_collect_reports_the_interpreter(self):
        lines, status = doctor.collect()
        self.assertEqual(status, 0)
        self.assertIn('interpreter: cpython 3.15.0', lines)
        self.assertTrue(any(line.startswith('executable: ') for line in lines))

    def test_free_threaded_build_is_rejected(self):
        original = doctor.sysconfig.get_config_var
        def config(name):
            return 1 if name == 'Py_GIL_DISABLED' else original(name)
        with mock.patch.object(doctor.sysconfig, 'get_config_var', side_effect=config):
            with self.assertRaisesRegex(doctor.DoctorError, 'free-threaded'):
                doctor.identity()

    def test_missing_uv_is_named(self):
        with self.assertRaisesRegex(doctor.DoctorError, 'missing uv 0.13.1'):
            doctor.uv_version({'PATH': '/nonexistent'})


if __name__ == '__main__':
    unittest.main()
