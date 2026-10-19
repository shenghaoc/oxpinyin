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

    def test_nextest_minimum_version(self):
        for version, accepted in (('0.9.114', False), ('0.9.115', True), ('0.10.0', True)):
            result = mock.Mock(returncode=0, stdout=f'cargo-nextest {version} (build)', stderr='')
            with self.subTest(version=version), \
                    mock.patch.object(doctor.shutil, 'which', return_value='/tools/cargo-nextest'), \
                    mock.patch.object(doctor, '_run', return_value=result):
                if accepted:
                    self.assertEqual(doctor.nextest_version({'PATH': '/tools'}), result.stdout)
                else:
                    with self.assertRaisesRegex(doctor.DoctorError, '0.9.115 or later required'):
                        doctor.nextest_version({'PATH': '/tools'})

    def test_missing_nextest_stays_optional(self):
        with mock.patch.object(doctor.shutil, 'which', return_value=None), \
                mock.patch.object(doctor, '_run') as run:
            self.assertIsNone(doctor.nextest_version({'PATH': '/tools'}))
        run.assert_not_called()

    def test_collect_reports_an_unsupported_nextest(self):
        info = dict(uv='uv 0.13.1', implementation='cpython', version='3.15.0',
                    managed=True, gil_enabled=True, executable='/managed/python')
        error = doctor.DoctorError('cargo-nextest 0.9.115 or later required; found 0.9.114')
        with mock.patch.object(doctor, 'host_lines', return_value=[]), \
                mock.patch.object(doctor, 'identity', return_value=info), \
                mock.patch.object(doctor, 'nextest_version', side_effect=error):
            lines, status = doctor.collect()
        self.assertEqual(status, 1)
        self.assertIn('FAIL: ' + str(error), lines)


if __name__ == '__main__':
    unittest.main()
