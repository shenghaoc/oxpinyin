#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""State ownership and driver patch boundaries of the extracted runner (#1175)."""
import contextlib
import importlib.util
import os
from pathlib import Path
import signal
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[1]))

import contract_runner


def load_driver():
    spec = importlib.util.spec_from_file_location('runner_boundary_driver', HERE / 'contract-diff.py')
    driver = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(driver)
    return driver


class StateTests(unittest.TestCase):
    def test_each_driver_load_has_one_independent_state_owner(self):
        first, second = load_driver(), load_driver()
        first._RUNNER.set_state(worker_env={'owned': 'first'}, worker_fds=(17,), cancelled=True)
        self.assertEqual(first.WORKER_ENV, {'owned': 'first'})
        self.assertEqual(first.WORKER_FDS, (17,))
        self.assertIs(first.CANCELLED, True)
        self.assertEqual(second.WORKER_ENV, {})
        self.assertEqual(second.WORKER_FDS, ())
        self.assertIs(second.CANCELLED, False)
        self.assertIsNot(first.WORKER_STATE, second.WORKER_STATE)
        self.assertIsNot(first.PLATFORM_SKIP, second.PLATFORM_SKIP)
        for name in contract_runner.STATE_NAMES:
            self.assertNotIn(name, vars(first), 'state must not have a second driver-global owner')

    def test_temporary_legacy_state_patches_reach_the_adapters_and_restore_the_owner(self):
        driver = load_driver()
        with patch.object(driver, 'CANCELLED', True), self.assertRaises(driver.Cancelled):
            driver.check_cancelled()
        driver.check_cancelled()
        with patch.dict(vars(driver), CANCELLED=True), self.assertRaises(driver.Cancelled):
            driver.check_cancelled()
        for name, value in (('WORKER_ENV', {'patch': 'yes'}), ('WORKER_FDS', (29,))):
            with self.subTest(name=name), patch.object(driver, name, value):
                self.assertIs(driver._RUNNER.setting(name), value)
        self.assertEqual(driver.WORKER_ENV, {})
        self.assertEqual(driver.WORKER_FDS, ())

    def test_handlers_use_the_owner_and_restore_previous_handlers(self):
        driver = load_driver()
        installed = {}
        previous = {number: object() for number in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP)}

        def install(number, handler):
            installed[number] = handler
            return previous[number]

        driver._RUNNER.set_state(cancelled=True)
        with patch.object(signal, 'signal', side_effect=install):
            with driver.cancellation_handlers():
                driver.check_cancelled()
                installed[signal.SIGTERM](signal.SIGTERM, None)
                with self.assertRaises(driver.Cancelled):
                    driver.check_cancelled()
            self.assertEqual(installed, previous)

    def test_admission_publishes_and_restores_only_owned_state_even_on_body_error(self):
        from tools.dev import slot
        driver = load_driver()
        old_env, old_fds = {'before': 'yes'}, (5,)
        driver._RUNNER.set_state(worker_env=old_env, worker_fds=old_fds)
        admission = SimpleNamespace(env={'OXPINYIN_CONTRACT_JOBS': '2'}, try_acquire=lambda: True,
                                    slot=SimpleNamespace(pass_fds=lambda: (19,), handoff=Mock()), close=Mock())
        with patch.dict(os.environ, OXPINYIN_HEAVY_SLOT_HELD='synthetic'), \
                patch.object(slot, 'HeavyAdmission', return_value=admission), \
                self.assertRaisesRegex(RuntimeError, 'body'):
            with driver.case_jobs(3) as admitted:
                self.assertEqual(admitted, 2)
                self.assertIs(driver.WORKER_ENV, admission.env)
                self.assertEqual(driver.WORKER_FDS, (19,))
                raise RuntimeError('body')
        self.assertIs(driver.WORKER_ENV, old_env)
        self.assertIs(driver.WORKER_FDS, old_fds)
        admission.close.assert_called_once_with()


class AdapterTests(unittest.TestCase):
    def test_driver_adapters_keep_their_namespaces_and_forward_to_their_own_runner(self):
        driver = load_driver()
        calls = (
            ('run_worker', ('pinyin', '/so', '/data', 'case', '/scratch'),
             ('pinyin', '/so', '/data', 'case', '/scratch', None, None, None)),
            ('run_case', ('spec', 'pin', 'subject', 'data', 'case', 'scratch', 10),
             ('spec', 'pin', 'subject', 'data', 'case', 'scratch', 10)),
            ('case_jobs', (2,), (2,)),
            ('case_results', ('args', 'names', 'data', 'scratch', 2), ('args', 'names', 'data', 'scratch', 2)),
        )
        for name, given, expected in calls:
            with self.subTest(name=name), patch.object(driver._RUNNER, name, return_value='forwarded') as moved:
                self.assertIs(getattr(driver, name).__globals__, vars(driver))
                self.assertEqual(getattr(driver, name)(*given), 'forwarded')
                moved.assert_called_once_with(*expected)

    def test_case_results_uses_a_new_driver_registry_and_run_case_patch_at_call_time(self):
        driver = load_driver()
        arguments = SimpleNamespace(prefix=Path('/pin'), cell='bdb', pinyin_so=Path('/subject'), case_timeout=1)
        registry = {'only': dict(mode='pinyin', cells=('bdb',))}
        with patch.object(driver, 'CASES', registry), patch.object(driver, 'run_case', return_value='pair') as run, \
                patch.object(contract_runner.tempfile, 'TemporaryDirectory', return_value=contextlib.nullcontext('/isolated')):
            with driver.case_results(arguments, ['only'], '/data', '/scratch', 1) as results:
                self.assertEqual(list(results), [('only', 'pair')])
        run.assert_called_once_with(registry['only'], Path('/pin/lib/libpinyin.so'), Path('/subject'),
                                    '/data', 'only', '/isolated', 1)


if __name__ == '__main__':
    unittest.main()
