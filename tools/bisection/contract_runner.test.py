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

import contract_reference
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


class ReferenceRunTests(unittest.TestCase):
    """capture runs the pin only and verify the subject only; the pin side of verify is the stored row."""

    def runner(self, worker):
        driver = dict(WORKER_STATE=SimpleNamespace(), run_worker=worker, selected_cell=lambda: 'tkrzw',
                      observed=lambda result: {k: v for k, v in result.items() if not k.startswith('~')},
                      tkrzw_saved_bigram_bytes=lambda raw: raw)
        return contract_runner.Runner(driver)

    def observation(self, **result):
        return dict(exit=0, result=result, userfiles={}, stderr_lines=0, stderr='')

    def test_verify_runs_only_the_subject_and_returns_the_stored_pin_observation(self):
        calls = []

        def worker(mode, so, data, name, scratch, extra_env=None, timeout=None):
            calls.append(Path(so).name)
            return self.observation(ret=1)

        spec = dict(mode='pinyin', fatal=False, pin_not_run=None)
        row = dict(pin=self.observation(ret=1))
        with tempfile_dir() as scratch:
            pin, subject = self.runner(worker).verify_case(spec, row, Path('/subject/libpinyin.so'), '/data', 'plain', scratch, 30)
        self.assertEqual(calls, ['libpinyin.so'])
        for key in ('capture_glibc', 'capture_glib', 'capture_dir_read_verdict', 'capture_scanf'):
            self.assertIsNone(pin.pop(key))
        self.assertEqual((pin, subject), (row['pin'], self.observation(ret=1)))

    def test_the_stored_pin_observation_carries_the_capture_glibc(self):
        spec = dict(mode='pinyin', fatal=False, pin_not_run=None)
        row = dict(pin=self.observation(ret=1), identity=dict(environment=dict(glibc='2.41')))
        with tempfile_dir() as scratch:
            pin, _ = self.runner(lambda *a, **k: self.observation(ret=1)).verify_case(
                spec, row, Path('/s.so'), '/data', 'plain', scratch, 30)
        self.assertEqual(pin['capture_glibc'], '2.41')

    def test_pin_not_run_is_never_taken_from_the_row(self):
        spec = dict(mode='pinyin', fatal=False, pin_not_run={'ret': 1})
        with tempfile_dir() as scratch:
            pin, _ = self.runner(lambda *a, **k: self.observation(ret=1)).verify_case(
                spec, dict(pin=self.observation(ret=9)), Path('/s.so'), '/data', 'plain', scratch, 30)
        self.assertEqual(pin['exit'], 'NOT-RUN')

    def test_capture_stores_the_profile_before_the_pin_reloads_it(self):
        # The stored profile is the one the pin wrote; the pin's own reload (which may rewrite the
        # container's clock field) comes after it is encoded.
        def worker(mode, so, data, name, scratch, extra_env=None, timeout=None):
            file = Path(extra_env['CONTRACT_929_PROFILE'], 'user_bigram.db')
            if extra_env.get('CONTRACT_929_RELOAD'):
                file.write_bytes(b'reopened')
            else:
                file.write_bytes(b'written')
            return self.observation(rows=[1])

        spec = dict(mode='pinyin', fatal=False, pin_not_run=None)
        with tempfile_dir() as scratch:
            observation, profile, _ = self.runner(worker).capture_case(
                spec, Path('/p.so'), '/data', 'save-929-logical-cross-open-pinyin', scratch, 30)
        self.assertEqual(observation['exit'], 0)
        with tempfile_dir() as out:
            contract_reference.decode_profile(profile, out)
            self.assertEqual(Path(out, 'user_bigram.db').read_bytes(), b'written')

    def test_capture_of_a_pin_not_run_case_starts_no_worker(self):
        def worker(*args, **kwargs):
            raise AssertionError('the pin must not run')

        spec = dict(mode='pinyin', fatal=False, pin_not_run={'ret': 1})
        with tempfile_dir() as scratch:
            observation, profile, used = self.runner(worker).capture_case(spec, Path('/p.so'), '/data', 'plain', scratch, 30)
        self.assertEqual((observation['exit'], profile, used), ('NOT-RUN', None, True))

    def test_lazy_bigram_capture_and_verify_compare_both_saved_snapshots(self):
        name = 'lazy-user-bigram-pinyin'
        spec = dict(mode='pinyin', fatal=False, pin_not_run=None)

        def writing(empty, trained):
            def worker(mode, so, data, case, scratch, extra_env=None, timeout=None):
                directory = Path(extra_env['CONTRACT_BIGRAM_SNAPSHOTS'])
                (directory / 'empty.db').write_bytes(empty)
                (directory / 'trained.db').write_bytes(trained)
                return self.observation(saved=True)
            return worker

        with tempfile_dir() as scratch:
            pin, profile, _ = self.runner(writing(b'empty', b'trained')).capture_case(
                spec, Path('/p.so'), '/data', name, scratch, 30)
        self.assertEqual(pin['result']['saved_byte_comparisons'], ['empty.db', 'trained.db'])
        row = dict(pin=pin, profile=profile)
        with tempfile_dir() as scratch:
            _, subject = self.runner(writing(b'empty', b'trained')).verify_case(
                spec, row, Path('/s.so'), '/data', name, scratch, 30)
        self.assertEqual(subject['result']['saved_byte_comparisons'], ['empty.db', 'trained.db'])
        for empty, trained, filename in ((b'changed', b'trained', 'empty.db'),
                                         (b'empty', b'changed', 'trained.db')):
            with self.subTest(filename=filename), tempfile_dir() as scratch:
                _, subject = self.runner(writing(empty, trained)).verify_case(
                    spec, row, Path('/s.so'), '/data', name, scratch, 30)
            self.assertEqual((subject['exit'], subject['result']),
                             (1, {'error': filename + ' differs outside META_OFFSET_TIMESTAMP'}))

    def test_a_handoff_case_lets_the_subject_read_the_pin_written_profile(self):
        name = 'save-929-logical-cross-open-pinyin'
        seen = []
        with tempfile_dir() as source:
            Path(source, 'user_bigram.db').write_bytes(b'pin-written')
            profile = contract_reference.encode_profile(source)

        def worker(mode, so, data, name, scratch, extra_env=None, timeout=None):
            seen.append(dict(extra_env or {}))
            if extra_env.get('CONTRACT_929_RELOAD') == '1':
                self.assertEqual(Path(extra_env['CONTRACT_929_PROFILE'], 'user_bigram.db').read_bytes(), b'pin-written')
                return self.observation(rows=[1])
            return self.observation(rows=[1])

        spec = dict(mode='pinyin', fatal=False, pin_not_run=None)
        row = dict(pin=self.observation(rows=[1], cross_reload={'rows': [1]}), profile=profile)
        with tempfile_dir() as scratch:
            _, subject = self.runner(worker).verify_case(spec, row, Path('/s.so'), '/data', name, scratch, 30)
        self.assertEqual(subject['result']['cross_reload'], {'rows': [1]})
        self.assertEqual(len(seen), 2)
        # A subject that reads the pin's profile differently fails the case.
        reloads = iter([self.observation(rows=[1]), self.observation(rows=[2])])
        with tempfile_dir() as scratch:
            _, subject = self.runner(lambda *a, **k: next(reloads)).verify_case(spec, row, Path('/s.so'), '/data', name, scratch, 30)
        self.assertEqual((subject['exit'], subject['result']), (1, {'error': 'cross-open changed logical records'}))


@contextlib.contextmanager
def tempfile_dir():
    import tempfile
    with tempfile.TemporaryDirectory() as directory:
        yield directory


if __name__ == '__main__':
    unittest.main()
