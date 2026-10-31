#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regressions for the contract differential's comparison and verdict rules (#1175).

These call the rules the way contract-diff.py does, with the findings
directory, the DBM cell and the directory-size probe passed in, so none of
them needs the driver or its registry. contract-diff.test.py keeps the checks
that go through the driver's own names."""

import json
import os
from pathlib import Path
import signal
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))

import contract_policy
import contract_verdict as verdict


def spec(**overrides):
    """A registration spec with the defaults of the driver's case_spec."""
    return dict(dict(mode='pinyin', control=False, abort=verdict.NO_ABORT, stderr=False, crash=None, userdir=False,
                     cells=('bdb', 'kc', 'tkrzw'), either=False, host_scanf=(), dir_chunk=None, stderr_varies=False,
                     abort_logs=None, fatal=False, both_abort=False, strjoinv_over_read=False, issues=(),
                     policy_record=None, case_id='probe', pin_not_run=None, sigabrt=False), **overrides)


def observation(exit=0, result=None, stderr='', userfiles=None):
    return dict(exit=exit, result=result, stderr=stderr, stderr_lines=len(stderr.splitlines()),
                userfiles={} if userfiles is None else userfiles)


def forbidden_probe():
    raise AssertionError('the directory probe was asked')


def decide(case, pin, subject, parent=False, **state):
    state = dict(dict(policy_root=Path('/nowhere'), cell='bdb', directory_probe=forbidden_probe), **state)
    return verdict.verdict_for(case, pin, subject, parent, **state)


class ObservationTests(unittest.TestCase):
    def test_observed_drops_only_top_level_tilde_keys(self):
        result = {'ret': 1, '~addr': 7, 'rows': {'~inner': 1}}
        self.assertEqual(verdict.observed(result), {'ret': 1, 'rows': {'~inner': 1}})
        self.assertIsNot(verdict.observed(result), result)
        self.assertIn('~addr', result)

    def test_an_ordinary_case_observed_something_only_after_a_normal_exit_with_a_result(self):
        for side, valid in ((observation(result={}), True), (observation(exit=1, result={}), False),
                            (observation(exit=-signal.SIGSEGV), False), (observation(), False),
                            (observation(exit='TIMEOUT'), False)):
            with self.subTest(exit=side['exit'], result=side['result']):
                self.assertIs(verdict.valid_observation(spec(), side), valid)

    def test_a_registered_termination_owns_its_exit_conditions(self):
        for overrides in (dict(abort=False), dict(crash={'ret': False}), dict(both_abort=True),
                          dict(pin_not_run={'ret': True})):
            with self.subTest(overrides=overrides):
                self.assertTrue(verdict.registered_termination(spec(**overrides)))
                self.assertTrue(verdict.valid_observation(spec(**overrides), observation(exit=-signal.SIGABRT)))
        self.assertFalse(verdict.registered_termination(spec()))


class MatchTests(unittest.TestCase):
    def matches(self, case, pin, subject, probe=forbidden_probe):
        return verdict.matches(case, pin, subject, directory_probe=probe)

    def test_an_ordinary_case_compares_everything_but_tilde_fields(self):
        pin = observation(result={'ret': 1, '~addr': 1})
        self.assertTrue(self.matches(spec(), pin, observation(result={'ret': 1, '~addr': 2})))
        self.assertFalse(self.matches(spec(), pin, observation(result={'ret': 2, '~addr': 1})))
        dead = observation(exit=-signal.SIGSEGV)
        self.assertFalse(self.matches(spec(), dead, dead), 'two identical failures observed nothing')

    def test_stderr_is_compared_only_when_the_case_asks(self):
        pin, subject = observation(result={}, stderr='a\n'), observation(result={}, stderr='b\n')
        self.assertTrue(self.matches(spec(), pin, subject))
        self.assertFalse(self.matches(spec(stderr=True), pin, subject))

    def test_an_abort_site_holds_when_the_subject_answers_with_one_warning_in_its_domain(self):
        pin = observation(exit=-signal.SIGABRT)
        case = spec(abort=False)
        answered = observation(result={'ret': False, 'logs': [['libpinyin', 16]], 'untouched out': True})
        self.assertTrue(self.matches(case, pin, answered))
        self.assertFalse(self.matches(case, pin, observation(result={'ret': True, 'logs': [['libpinyin', 16]]})))
        self.assertFalse(self.matches(case, pin, observation(result={'ret': False, 'logs': [['libzhuyin', 16]]})))
        self.assertFalse(self.matches(case, pin, observation(result={'ret': False, 'logs': [['libpinyin', 16]],
                                                                     'untouched out': False})))
        self.assertTrue(self.matches(spec(abort=False, mode='zhuyin'), pin,
                                     observation(result={'ret': False, 'logs': [['libzhuyin', 16]]})))
        self.assertTrue(self.matches(spec(abort=False, abort_logs=[]), pin, observation(result={'ret': False, 'logs': []})))
        self.assertFalse(self.matches(case, observation(exit=-signal.SIGSEGV), answered))

    def test_a_reproduced_abort_needs_sigabrt_on_both_sides_and_the_pins_stderr_less_its_assert_line(self):
        line = "python3: src/pinyin.cpp:1474: bool f(): Assertion `index < results.size()' failed.\n"
        pin = observation(exit=-signal.SIGABRT, stderr='mmap a failed!\n' + line)
        case = spec(abort=False, sigabrt=True)
        self.assertTrue(self.matches(case, pin, observation(exit=-signal.SIGABRT, stderr='mmap a failed!\n')))
        self.assertFalse(self.matches(case, pin, observation(exit=-signal.SIGABRT)), 'earlier stderr is compared')
        self.assertFalse(self.matches(case, pin, observation(result={'ret': False, 'logs': [['libpinyin', 16]]})),
                         'the refusal no longer holds the site')
        self.assertFalse(self.matches(case, pin, observation(exit=-signal.SIGSEGV, stderr='mmap a failed!\n')))
        self.assertFalse(self.matches(case, observation(exit=-signal.SIGSEGV), observation(exit=-signal.SIGABRT)))
        silent = observation(exit=-signal.SIGABRT)
        self.assertTrue(self.matches(case, silent, silent), 'an abort() site writes nothing on either side')
        profile = spec(abort=False, sigabrt=True, userdir=True)
        self.assertFalse(self.matches(profile, observation(exit=-signal.SIGABRT, userfiles={'user.conf': 'a'}), silent))
        self.assertTrue(verdict.stderr_less_assert_line('x\n', 'x\n'))
        self.assertFalse(verdict.stderr_less_assert_line('x\ny\n', 'x\n'), 'only an assertion line may trail')
        # `open %s failed.` ends without a newline, and glibc's line follows
        # it on the same line.
        joined = 'open u/user.conf failed.' + line
        self.assertTrue(self.matches(case, observation(exit=-signal.SIGABRT, stderr=joined),
                                     observation(exit=-signal.SIGABRT, stderr='open u/user.conf failed.')))
        self.assertFalse(self.matches(case, observation(exit=-signal.SIGABRT, stderr=joined),
                                      observation(exit=-signal.SIGABRT)), 'the joined text is compared')

    def test_a_crash_site_needs_sigsegv_the_same_stderr_and_the_declared_fields(self):
        case = spec(crash={'ret': False}, stderr=True)
        pin = observation(exit=-signal.SIGSEGV, stderr='x\n')
        self.assertTrue(self.matches(case, pin, observation(result={'ret': False}, stderr='x\n')))
        self.assertFalse(self.matches(case, pin, observation(result={'ret': False}, stderr='y\n')))
        self.assertFalse(self.matches(case, pin, observation(result={'ret': True}, stderr='x\n')))
        self.assertTrue(self.matches(spec(crash={'ret': False}, stderr_varies=True), pin,
                                     observation(result={'ret': False}, stderr='anything')))

    def test_either_lets_the_pins_own_signal_choose_the_matcher(self):
        case = spec(abort=False, crash={'ret': False}, either=True, stderr=True)
        self.assertTrue(self.matches(case, observation(exit=-signal.SIGABRT),
                                     observation(result={'ret': False, 'logs': [['libpinyin', 16]]})))
        self.assertTrue(self.matches(case, observation(exit=-signal.SIGSEGV, stderr='x\n'),
                                     observation(result={'ret': False}, stderr='x\n')))

    def test_a_pin_that_was_not_run_holds_the_subject_to_its_declaration(self):
        case = spec(pin_not_run={'ret': True})
        not_run = observation(exit='NOT-RUN')
        self.assertTrue(self.matches(case, not_run, observation(result={'ret': True, 'logs': []})))
        self.assertFalse(self.matches(case, not_run, observation(result={'ret': False, 'logs': []})))
        self.assertFalse(self.matches(case, observation(result={'ret': True}), observation(result={'ret': True})))

    def test_both_abort_compares_the_signal_and_stderr_without_process_id_or_time(self):
        case = spec(both_abort=True, fatal=True, stderr=True)
        text = '** (process:%d): CRITICAL **: 00:00:0%d.001: boom\n'
        pin = observation(exit=-signal.SIGABRT, stderr=text % (1, 1))
        self.assertTrue(self.matches(case, pin, observation(exit=-signal.SIGABRT, stderr=text % (2, 2))))
        self.assertFalse(self.matches(case, pin, observation(exit=-signal.SIGABRT, stderr=(text % (2, 2)).replace('boom', 'bang'))))
        self.assertFalse(self.matches(case, pin, observation(exit=-signal.SIGTRAP, stderr=text % (2, 2))))
        self.assertFalse(self.matches(case, pin, observation(result={'ret': True}, stderr=text % (2, 2))))
        self.assertFalse(self.matches(case, observation(exit=-signal.SIGSEGV), observation(exit=-signal.SIGSEGV)))

    def test_fatal_stderr_keeps_severity_and_message(self):
        self.assertEqual(verdict.fatal_stderr('** (process:4242): WARNING **: 12:34:56.789: low\nplain\n'),
                         '** WARNING **: low\nplain\n')

    def test_a_policy_record_never_counts_as_a_match(self):
        same = observation(result={'ret': 1})
        self.assertFalse(self.matches(spec(policy_record='r.md'), same, same))

    def test_a_held_strjoinv_fault_is_a_match_and_a_completed_pin_is_compared_exactly(self):
        case = spec(strjoinv_over_read=True)
        pin = observation(exit=-signal.SIGSEGV, stderr='e\n')
        done = observation(result={'rows': []}, stderr='e\n')
        self.assertTrue(verdict.strjoinv_fault(pin, done))
        self.assertTrue(self.matches(case, pin, done))
        self.assertFalse(self.matches(case, pin, observation(result={'rows': []}, stderr='other\n')))
        self.assertTrue(self.matches(case, done, done))
        self.assertFalse(self.matches(case, done, observation(result={'rows': [1]}, stderr='e\n')))

    def test_the_directory_probe_is_asked_only_for_a_case_that_registers_a_dir_chunk(self):
        pin = observation(result={'ret': 1}, stderr='s', userfiles={'a': 1})
        self.assertTrue(self.matches(spec(), pin, dict(pin)))
        asked = []

        def reads(answer):
            return lambda: asked.append(answer) or answer

        # The host reads the directory as a chunk: the pin reached its assert, so the abort matcher applies.
        abort = spec(abort=False, dir_chunk='user')
        survivor = observation(exit=-signal.SIGABRT)
        answered = observation(result={'ret': False, 'logs': [['libpinyin', 16]]})
        self.assertTrue(self.matches(abort, survivor, answered, reads(True)))
        # It does not: both sides carry on and are compared as ordinary results, with stderr and profile.
        self.assertTrue(self.matches(abort, pin, dict(pin), reads(False)))
        self.assertFalse(self.matches(abort, pin, dict(pin, userfiles={}), reads(False)))
        self.assertEqual(asked, [True, False, False])

    def test_a_system_dir_chunk_on_a_short_host_holds_the_pins_sigsegv_and_the_abort_answer(self):
        case = spec(abort=False, dir_chunk='system')
        pin = observation(exit=-signal.SIGSEGV, stderr='mmap failed!\n')
        answered = observation(result={'ret': False, 'logs': [['libpinyin', 16]]}, stderr='mmap failed!\n')
        self.assertTrue(self.matches(case, pin, answered, lambda: False))
        self.assertFalse(self.matches(case, pin, dict(answered, stderr='x'), lambda: False))
        self.assertFalse(self.matches(case, pin, answered, lambda: True), 'a host that reads it expects the abort')


class VerdictTests(unittest.TestCase):
    def test_a_difference_counts_only_between_two_valid_observations(self):
        good, other = observation(result={'ret': 1}), observation(result={'ret': 2})
        self.assertEqual(decide(spec(), good, good), ('MATCH', True, True))
        self.assertEqual(decide(spec(), good, other), ('DIFFER', False, False))
        for bad in (observation(exit=1), observation(exit='TIMEOUT'), observation()):
            for pin, subject in ((bad, good), (good, bad), (bad, bad)):
                self.assertEqual(decide(spec(), pin, subject), ('INVALID', False, False))
                self.assertEqual(decide(spec(), pin, subject, True)[:2], ('INVALID', False))

    def test_expect_parent_wants_a_difference_from_every_case_that_is_not_a_control(self):
        good, other = observation(result={'ret': 1}), observation(result={'ret': 2})
        self.assertEqual(decide(spec(), good, good, True), ('MATCH', False, True))
        self.assertEqual(decide(spec(control=True), good, good, True), ('MATCH', True, True))
        self.assertEqual(decide(spec(), good, other, True), ('DIFFER', True, False))
        self.assertEqual(decide(spec(control=True), good, other, True), ('DIFFER', False, False))

    def test_a_reproduced_assert_is_policy_accepted_and_a_reproduced_abort_call_matches(self):
        line = "python3: a.cpp:1: f: Assertion `x' failed.\n"
        case, dead = spec(abort=False, sigabrt=True), observation(exit=-signal.SIGABRT)
        self.assertEqual(decide(case, observation(exit=-signal.SIGABRT, stderr=line), dead),
                         ('POLICY-ACCEPTED', True, True))
        self.assertEqual(decide(case, dead, dead), ('MATCH', True, True))
        refused = observation(result={'ret': False, 'logs': [['libpinyin', 16]]})
        self.assertEqual(decide(case, dead, refused), ('DIFFER', False, False))
        self.assertEqual(decide(case, dead, refused, True), ('DIFFER', True, False), 'the parent refused')

    def test_parent_rejects_unrelated_subject_failures_at_registered_terminations(self):
        for case, pin in ((spec(abort=False), observation(exit=-signal.SIGABRT)),
                          (spec(crash={'ret': False}), observation(exit=-signal.SIGSEGV)),
                          (spec(both_abort=True, fatal=True), observation(exit=-signal.SIGTRAP)),
                          (spec(abort=False, crash={'ret': False}, either=True),
                           observation(exit=-signal.SIGABRT))):
            for bad in (observation(exit=-signal.SIGILL), observation(exit='TIMEOUT'),
                        observation(exit=127, stderr='loader failed'), observation(),
                        observation(exit=1, result={'ret': True})):
                for control in (False, True):
                    with self.subTest(case=case, bad=bad, control=control):
                        self.assertEqual(decide(dict(case, control=control), pin, bad, True),
                                         ('INVALID', False, False))

    def test_parent_rejects_a_pin_that_violates_its_registered_termination(self):
        for case, wrong_signal in ((spec(abort=False), -signal.SIGSEGV),
                                   (spec(crash={'ret': False}), -signal.SIGABRT),
                                   (spec(both_abort=True, fatal=True), -signal.SIGSEGV),
                                   (spec(abort=False, crash={'ret': False}, either=True), -signal.SIGILL)):
            for pin in (observation(exit=wrong_signal), observation(exit='TIMEOUT'),
                        observation(exit=127), observation(), observation(result={'ret': False})):
                with self.subTest(case=case, pin=pin):
                    self.assertEqual(decide(case, pin, observation(result={'ret': True}), True),
                                     ('INVALID', False, False))

    def test_parent_rejects_the_wrong_registered_kind_of_subject_termination(self):
        for case, pin_exit, wrong_exit in ((spec(abort=False), -signal.SIGABRT, -signal.SIGSEGV),
                                           (spec(crash={'ret': False}), -signal.SIGSEGV, -signal.SIGABRT),
                                           (spec(both_abort=True, fatal=True), -signal.SIGTRAP, -signal.SIGABRT),
                                           (spec(both_abort=True, fatal=True), -signal.SIGABRT, -signal.SIGTRAP)):
            with self.subTest(case=case, pin_exit=pin_exit, wrong_exit=wrong_exit):
                self.assertEqual(decide(case, observation(exit=pin_exit), observation(exit=wrong_exit), True),
                                 ('INVALID', False, False))

    def test_parent_accepts_completed_regressions_and_registered_subject_terminations(self):
        for case, exits in ((spec(abort=False), (-signal.SIGABRT,)),
                            (spec(crash={'ret': False}), (-signal.SIGSEGV,)),
                            (spec(abort=False, crash={'ret': False}, either=True),
                             (-signal.SIGABRT, -signal.SIGSEGV))):
            for pin_exit in exits:
                pin = observation(exit=pin_exit)
                for subject in (observation(result={'ret': True, 'logs': []}), observation(exit=pin_exit)):
                    with self.subTest(case=case, pin_exit=pin_exit, subject=subject):
                        self.assertEqual(decide(case, pin, subject, True), ('DIFFER', True, False))
                        self.assertEqual(decide(dict(case, control=True), pin, subject, True),
                                         ('DIFFER', False, False))
        for pin_exit in (-signal.SIGABRT, -signal.SIGTRAP):
            case = spec(both_abort=True, fatal=True, stderr=True)
            pin = observation(exit=pin_exit, stderr='fatal message')
            self.assertEqual(decide(case, pin, observation(result={}), True), ('DIFFER', True, False))
            self.assertEqual(decide(case, pin, observation(exit=pin_exit, stderr='wrong message'), True),
                             ('DIFFER', True, False))
            self.assertEqual(decide(case, pin, dict(pin), True), ('MATCH', False, True))

    def test_parent_directory_chunk_validity_uses_the_registered_host_path(self):
        for kind in ('user', 'system'):
            case = spec(abort=False, dir_chunk=kind)
            for reads in (False, True):
                pin = (observation(exit=-signal.SIGABRT) if reads else
                       observation(exit=-signal.SIGSEGV) if kind == 'system' else observation(result={}))
                state = dict(directory_probe=lambda: reads)
                with self.subTest(kind=kind, reads=reads):
                    self.assertEqual(decide(case, pin, observation(result={'ret': True}), True, **state),
                                     ('DIFFER', True, False))
                    self.assertEqual(decide(case, pin, observation(exit=-signal.SIGILL), True, **state),
                                     ('INVALID', False, False))
                    self.assertEqual(decide(case, observation(exit=-signal.SIGILL),
                                            observation(result={}), True, **state), ('INVALID', False, False))
                    if pin['exit'] != 0:
                        self.assertEqual(decide(case, pin, dict(pin), True, **state), ('DIFFER', True, False))

    def test_pin_not_run_classification_is_unchanged_in_both_modes(self):
        case = spec(pin_not_run={'ret': True})
        pin = observation(exit='NOT-RUN')
        for subject, same in ((observation(result={'ret': True}), True),
                              (observation(result={'ret': False}), False),
                              (observation(exit=-signal.SIGSEGV), False),
                              (observation(exit='TIMEOUT'), False), (observation(), False)):
            for parent in (False, True):
                with self.subTest(subject=subject, parent=parent):
                    self.assertEqual(decide(case, pin, subject, parent),
                                     ('MATCH' if same else 'DIFFER', not same if parent else same, same))

    def test_held_strjoinv_pin_fault_stays_valid_in_both_modes(self):
        case = spec(strjoinv_over_read=True)
        pin = observation(exit=-signal.SIGSEGV, stderr='e\n')
        subject = observation(result={'rows': []}, stderr='e\n')
        self.assertEqual(decide(case, pin, subject), ('MATCH', True, True))
        self.assertEqual(decide(case, pin, subject, True), ('MATCH', False, True))
        self.assertEqual(decide(case, pin, dict(subject, stderr='wrong'), True), ('INVALID', False, False))


class PolicyTests(unittest.TestCase):
    PIN = dict(exit=0, result={'ret': True, 'rows': [[2, 'e794b2']], 'logs': []}, stderr='')
    SUBJECT = dict(exit=0, result={'ret': True, 'rows': [], 'logs': []}, stderr='')

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / 'upstream-divergences.md').write_text('[record](rec.md)\n')
        self.spec = spec(policy_record='rec.md', case_id='family/case')
        self.write(dict(pin=self.PIN, subject=self.SUBJECT))

    def write(self, pair, **metadata):
        """Record `pair` ({'pin': obs, 'subject': obs}, or {'cells': {cell: pair}}) in the digest form."""
        if set(pair) == {'cells'}:
            kept = {'cells': {cell: contract_policy.entry(p['pin'], p['subject']) for cell, p in pair['cells'].items()}}
        else:
            kept = contract_policy.entry(pair['pin'], pair['subject'])
        self.write_entry(kept, **metadata)

    def write_entry(self, kept, schema=contract_policy.SCHEMA, **metadata):
        record = dict(schema=schema, record='rec', status='current', disposition='POLICY-ACCEPTED',
                      cases={'family/case': kept}, **metadata)
        (self.root / 'rec.md').write_text('# Record\n\n```contract-diff-policy\n' + json.dumps(record) + '\n```\n')

    def decide(self, pin=None, subject=None, parent=False, **state):
        return decide(self.spec, pin or dict(self.PIN, stderr_lines=0, userfiles={}),
                      subject or dict(self.SUBJECT, stderr_lines=0, userfiles={}), parent,
                      **dict(dict(policy_root=self.root), **state))

    def test_both_recorded_observations_are_accepted_in_either_mode(self):
        for parent in (False, True):
            self.assertEqual(self.decide(parent=parent), ('POLICY-ACCEPTED', True, False))

    def test_a_registered_policy_abort_does_not_exempt_invalid_parent_workers(self):
        self.spec['abort'] = False
        pin = dict(self.PIN, exit=-signal.SIGABRT, result=None)
        self.write(dict(pin=pin, subject=self.SUBJECT))
        self.assertEqual(self.decide(pin=pin, parent=True), ('POLICY-ACCEPTED', True, False))
        for bad in (dict(self.SUBJECT, exit=-signal.SIGILL, result=None),
                    dict(self.SUBJECT, exit='TIMEOUT', result=None), dict(self.SUBJECT, result=None)):
            with self.subTest(subject=bad):
                self.assertEqual(self.decide(pin=pin, subject=bad, parent=True), ('INVALID', False, False))

    def test_any_other_observation_is_a_difference_and_a_missing_record_is_invalid(self):
        changed = dict(self.SUBJECT, result=dict(self.SUBJECT['result'], ret=False), stderr_lines=0, userfiles={})
        self.assertEqual(self.decide(subject=changed), ('DIFFER', False, False))
        self.assertEqual(self.decide(policy_root=self.root / 'missing'), ('INVALID', False, False))
        (self.root / 'upstream-divergences.md').write_text('')
        self.assertEqual(self.decide(), ('INVALID', False, False))

    def test_the_record_is_read_from_the_root_it_is_given_and_names_the_cell_it_is_given(self):
        pair = dict(pin=self.PIN, subject=self.SUBJECT)
        self.write({'cells': {'kc': pair}})
        with patch.dict(os.environ):
            os.environ.pop('CONTRACT_DIFF_CELL', None)
            self.assertEqual(self.decide(cell='kc'), ('POLICY-ACCEPTED', True, False))
            self.assertEqual(self.decide(cell='bdb'), ('INVALID', False, False))
        with patch.dict(os.environ, CONTRACT_DIFF_CELL='kc'):
            self.assertEqual(self.decide(cell='bdb'), ('POLICY-ACCEPTED', True, False), 'the environment names the cell first')

    def test_a_record_must_stay_inside_the_findings_directory_and_be_registered(self):
        for name in ('../rec.md', 'sub/rec.md', 'rec.txt'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                verdict.policy_expectations(dict(self.spec, policy_record=name), policy_root=self.root, cell='bdb')
        (self.root / 'upstream-divergences.md').write_text('[other](other.md)\n')
        with self.assertRaisesRegex(ValueError, 'not registered'):
            verdict.policy_expectations(self.spec, policy_root=self.root, cell='bdb')

    def test_a_record_that_records_parity_is_refused(self):
        kept = contract_policy.entry(self.PIN, self.SUBJECT)
        kept['subject'] = kept['pin']
        self.write_entry(kept)
        with self.assertRaisesRegex(ValueError, 'difference, not parity'):
            verdict.policy_expectations(self.spec, policy_root=self.root, cell='bdb')

    def test_a_schema_1_record_of_complete_observations_is_refused_by_name(self):
        self.write_entry(dict(pin=self.PIN, subject=self.SUBJECT), schema=1)
        with self.assertRaisesRegex(ValueError, 'schema 1.*policy-record'):
            verdict.policy_expectations(self.spec, policy_root=self.root, cell='bdb')
        self.assertEqual(self.decide(), ('INVALID', False, False))
        self.write_entry(contract_policy.entry(self.PIN, self.SUBJECT), schema=3)
        self.assertEqual(self.decide(), ('INVALID', False, False))

    def test_a_malformed_entry_is_invalid(self):
        kept = contract_policy.entry(self.PIN, self.SUBJECT)
        for broken in (dict(kept, divergence=[]), {k: v for k, v in kept.items() if k != 'subject'},
                       dict(kept, pin=dict(kept['pin'], sha256='short')), dict(kept, pin={'sha256': kept['pin']['sha256']})):
            with self.subTest(broken=broken):
                self.write_entry(broken)
                self.assertEqual(self.decide(), ('INVALID', False, False))

    def test_an_accepted_pair_must_still_be_one_the_record_could_hold(self):
        pin = dict(self.PIN, result={'ret': True})  # no logs
        self.write(dict(pin=pin, subject=self.SUBJECT))
        self.assertEqual(self.decide(pin=dict(pin, stderr_lines=0, userfiles={})), ('INVALID', False, False))

    def test_the_diagnosis_names_the_side_and_the_fields_that_moved(self):
        moved = dict(self.SUBJECT, result=dict(self.SUBJECT['result'], ret=False, extra=1), stderr_lines=0, userfiles={})
        report = verdict.policy_diagnosis(self.spec, dict(self.PIN, stderr_lines=0, userfiles={}), moved,
                                          policy_root=self.root, cell='bdb')
        self.assertEqual(report['summary'], 'pin matches; subject moved in result.ret, +result.extra')
        self.assertTrue(report['pin']['matches'])
        self.assertEqual(report['subject'], dict(matches=False, moved=['result.ret'], added=['result.extra'], missing=[]))
        self.assertIsNone(verdict.policy_diagnosis(self.spec, dict(self.PIN, stderr_lines=0, userfiles={}),
                                                   dict(self.SUBJECT, stderr_lines=0, userfiles={}),
                                                   policy_root=self.root, cell='bdb'))
        self.assertIsNone(verdict.policy_diagnosis(self.spec, self.PIN, moved, policy_root=self.root / 'missing', cell='bdb'))

    def test_a_recorded_observation_holds_exactly_the_keys_the_case_compares(self):
        self.assertEqual(verdict.policy_observation(self.spec, dict(self.PIN, userfiles={'a': 'b'}, stderr_lines=3)),
                         self.PIN)
        self.assertEqual(verdict.policy_observation(dict(self.spec, userdir=True), dict(self.PIN, userfiles={'a': 'b'})),
                         dict(self.PIN, userfiles={'a': 'b'}))


class HostTests(unittest.TestCase):
    def test_a_pin_that_loaded_beside_a_subject_that_refused_table_conf_is_recognised_exactly(self):
        pin = observation(result={'ctx': True}, stderr='open /s/user-X/user.conf failed.')
        subject = observation(result={'ctx': False}, stderr='load /s/sys-X/table.conf failed!\n')
        self.assertTrue(verdict.pin_loaded_subject_refused_table_conf(pin, subject))
        for pin_change, subject_change in ((dict(exit=1), {}), ({}, dict(exit=1)),
                                           (dict(result={'ctx': False}), {}), ({}, dict(result={'ctx': True})),
                                           (dict(stderr='open /s/user.conf failed.\n'), {}),
                                           ({}, dict(stderr='load /s/table.conf failed!')),
                                           ({}, dict(stderr='load /s/table.conf failed!\nmore'))):
            with self.subTest(pin=pin_change, subject=subject_change):
                self.assertFalse(verdict.pin_loaded_subject_refused_table_conf(
                    dict(pin, **pin_change), dict(subject, **subject_change)))

    @unittest.skipUnless(sys.platform.startswith('linux'), 'the probe calls the host libc sscanf through ctypes')
    def test_host_scanf_accepts_a_float_prefix(self):
        for spelling, accepted in (('1.5', True), ('1e3xyz', True), ('abc', False), ('', False)):
            with self.subTest(spelling=spelling):
                self.assertIs(verdict.host_scanf_accepts(spelling), accepted)

    def test_the_directory_probe_repeats_the_pins_eight_byte_size_gate_and_caches_its_answer(self):
        probe = verdict.directory_reads_as_a_chunk
        for sought, reads in ((OSError(22, 'Invalid argument'), False), (7, False), (8, True)):
            with self.subTest(sought=sought):
                probe.cache_clear()
                with patch.object(verdict.os, 'lseek', side_effect=[sought]):
                    self.assertIs(probe(), reads)
                    self.assertIs(probe(), reads)
        probe.cache_clear()

    def test_the_saved_bigram_mask_covers_only_the_timestamp_bytes(self):
        raw = bytes(range(256))
        masked = verdict.tkrzw_saved_bigram_bytes(raw)
        self.assertEqual((verdict.META_OFFSET_TIMESTAMP, verdict.TKRZW_TIMESTAMP_WIDTH), (48, 8))
        self.assertEqual(masked[48:56], bytes(8))
        self.assertEqual(masked[:48] + masked[56:], raw[:48] + raw[56:])
        with self.assertRaisesRegex(ValueError, 'metadata'):
            verdict.tkrzw_saved_bigram_bytes(raw[:verdict.TKRZW_HASH_METADATA_SIZE - 1])
        chains = ((5, (b'a', b'b')),)
        self.assertTrue(verdict.tkrzw_saved_bigram_equal(raw, raw, chains, chains))
        self.assertFalse(verdict.tkrzw_saved_bigram_equal(raw, raw, chains, ((5, (b'b', b'a')),)))


if __name__ == '__main__':
    unittest.main()
