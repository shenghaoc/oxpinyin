#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regressions for the frozen pin references of contract-diff (contract_reference)."""

import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

import contract_reference as ref
from contract_verdict import NO_ABORT


def case():
    return {}


def spec(**changes):
    return dict(dict(fn=case, case_id='demo', mode='pinyin', abort=NO_ABORT, pin_not_run=None), **changes)


def row(s, **changes):
    value = dict(schema=ref.SCHEMA, case='demo', cell='tkrzw', provenance='pin',
                 identity=dict(inputs=ref.inputs_digest(s, NO_ABORT), pin=dict(commit=ref.PIN_COMMIT),
                               backend='tkrzw', system_data=dict(kind=ref.SYNTHETIC),
                               environment=dict(image='sha256:x', glib='2.84.4', glibc='2.41')),
                 pin=dict(exit=0, result={}, userfiles={}, stderr_lines=0, stderr=''))
    value.update(changes)
    return value


class IdentityTests(unittest.TestCase):
    def test_inputs_digest_keeps_an_option_set_to_zero(self):
        # `0 == False`: an option at 0 or 0.0 is an input, only None, False and empty containers are defaults.
        self.assertNotEqual(ref.inputs_digest(spec(abort=0), NO_ABORT), ref.inputs_digest(spec(abort=None), NO_ABORT))
        self.assertNotEqual(ref.inputs_digest(spec(abort=0), NO_ABORT), ref.inputs_digest(spec(abort=False), NO_ABORT))
        self.assertNotEqual(ref.inputs_digest(spec(abort=0.0), NO_ABORT), ref.inputs_digest(spec(abort=None), NO_ABORT))
        self.assertEqual(ref.inputs_digest(spec(issues=()), NO_ABORT), ref.inputs_digest(spec(issues=[]), NO_ABORT))

    def test_inputs_digest_follows_the_case_and_not_the_address_of_the_sentinel(self):
        self.assertEqual(ref.inputs_digest(spec(), NO_ABORT), ref.inputs_digest(spec(), NO_ABORT))
        self.assertNotEqual(ref.inputs_digest(spec(), NO_ABORT), ref.inputs_digest(spec(stderr=True), NO_ABORT))
        self.assertNotEqual(ref.inputs_digest(spec(), NO_ABORT), ref.inputs_digest(dict(spec(), case_id='other'), NO_ABORT))

    def test_data_identity_names_content_and_needs_a_table_conf(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ref.ReferenceError):
                ref.data_identity(directory)
            Path(directory, 'table.conf').write_text('x')
            first = ref.data_identity(directory)
            Path(directory, 'a.bin').write_bytes(b'1')
            self.assertNotEqual(first, ref.data_identity(directory))


class DigestTests(unittest.TestCase):
    def digest(self, source):
        return ref.sha256_text(ref._code(source))

    def test_comments_blank_lines_formatting_and_docstrings_are_not_inputs(self):
        base = "def probe(k):\n    return {'ret': k.fn('x')(1, 2)}\n"
        edited = ("def probe(k):\n    \"\"\"A docstring.\"\"\"\n    # a comment\n\n"
                  "    return {  'ret':  k.fn( 'x' )( 1,\n 2 ) }  # trailing\n")
        self.assertEqual(self.digest(base), self.digest(edited))

    def test_code_edits_change_the_digest(self):
        base = "def probe(k):\n    return {'ret': k.fn('x')(1, 2)}\n"
        for changed in ("def probe(k):\n    return {'ret': k.fn('x')(1, 3)}\n",
                        "def probe(k):\n    return {'ret2': k.fn('x')(1, 2)}\n",
                        "def probe(k):\n    k.init()\n    return {'ret': k.fn('x')(1, 2)}\n"):
            self.assertNotEqual(self.digest(base), self.digest(changed))

    def test_a_fragment_from_a_decorator_call_is_digested_by_its_tokens(self):
        a = "stderr=True, issues=('922',))(lambda k, system=_s: line(k, system))\n"
        b = "stderr=True,   issues=('922',))(lambda k, system=_s: line(k, system))  # note\n"
        self.assertEqual(ref.sha256_text(ref._code(a)), ref.sha256_text(ref._code(b)))
        self.assertNotEqual(ref.sha256_text(ref._code(a)), ref.sha256_text(ref._code(a.replace('922', '923'))))

    def test_a_policy_record_is_not_an_input(self):
        spec = dict(fn=lambda k: 1, case_id='x', mode='pinyin', abort=NO_ABORT, pin_not_run=None)
        self.assertEqual(ref.inputs_digest(spec, NO_ABORT), ref.inputs_digest(dict(spec, policy_record='r.md'), NO_ABORT))

    def test_a_changed_closure_bound_input_changes_the_digest(self):
        def factory(text):
            def probe():
                return text
            return probe
        first = spec(fn=factory('alpha'))
        second = spec(fn=factory('beta'))
        self.assertNotEqual(ref.inputs_digest(first, NO_ABORT), ref.inputs_digest(second, NO_ABORT))

    def test_a_checkout_local_path_has_the_same_digest_in_another_checkout(self):
        from unittest.mock import patch
        def factory(worker):
            def probe():
                return worker
            return probe
        with patch.object(ref, 'ROOT', Path('/tmp/checkout-a')):
            first = ref.inputs_digest(spec(fn=factory(Path('/tmp/checkout-a/tools/worker.py'))), NO_ABORT)
        with patch.object(ref, 'ROOT', Path('/tmp/checkout-b')):
            second = ref.inputs_digest(spec(fn=factory(Path('/tmp/checkout-b/tools/worker.py'))), NO_ABORT)
        self.assertEqual(first, second)


class CheckRowTests(unittest.TestCase):
    def check(self, s, value, digest=None):
        ref.check_row(value, s, 'tkrzw', digest, NO_ABORT)

    def test_a_valid_row_passes(self):
        s = spec()
        self.check(s, row(s))

    def test_each_incompatibility_is_named(self):
        s = spec()
        good = row(s)
        for field, mutate, text in (
                ('schema', lambda v: v.update(schema=9), 'schema'),
                ('inputs', lambda v: v['identity'].update(inputs='0'), 'case changed'),
                ('pin', lambda v: v['identity']['pin'].update(commit='x'), 'not 074a2219'),
                ('backend', lambda v: v['identity'].update(backend='bdb'), 'backend'),
                ('data', lambda v: v['identity'].pop('system_data'), 'system-data'),
                ('image', lambda v: v['identity']['environment'].pop('image'), 'capture image'),
                ('pin observation', lambda v: v['pin'].pop('stderr'), 'complete pin observation')):
            with self.subTest(field):
                value = json.loads(json.dumps(good))
                mutate(value)
                with self.assertRaisesRegex(ref.ReferenceError, text):
                    self.check(s, value)

    def test_package_data_must_match_the_digest_of_this_run(self):
        s = spec()
        value = row(s)
        value['identity']['system_data'] = dict(kind=ref.PACKAGE, digest='aa')
        self.check(s, value, 'aa')
        with self.assertRaisesRegex(ref.ReferenceError, 'system data aa'):
            self.check(s, value, 'bb')

    def test_pin_not_run_keeps_its_provenance(self):
        s = spec(pin_not_run={'ret': 1})
        value = row(s, provenance='pin_not_run', pin=dict(ref.NOT_RUN))
        self.check(s, value)
        with self.assertRaisesRegex(ref.ReferenceError, 'pin_not_run reference'):
            self.check(s, row(s))
        with self.assertRaisesRegex(ref.ReferenceError, 'does not hold a pin observation'):
            self.check(spec(), row(spec(), provenance='pin_not_run', pin=dict(ref.NOT_RUN)))

    def test_handoff_requires_a_profile(self):
        s = spec(case_id='save-929-logical-cross-open-pinyin')
        value = row(s, case=s['case_id'])
        with self.assertRaisesRegex(ref.ReferenceError, 'handoff profile'):
            self.check(s, value)

    def test_lambda_reference_requires_its_captured_scanner_verdict(self):
        def probe():
            pass
        s = spec(case_id='table-conf-lambda-probe')
        s['fn'] = probe
        s['fn'].config_edits = (('lambda parameter:0.312699', 'lambda parameter:1e'),)
        value = row(s, case=s['case_id'])
        with self.assertRaisesRegex(ref.ReferenceError, 'capture scanner verdict'):
            self.check(s, value)
        value['scanner_verdict'] = dict(input='1e', accepted=False, value_bits=None)
        self.check(s, value)


class StoreTests(unittest.TestCase):
    def test_rows_merge_sorted_and_a_missing_one_is_an_error_naming_the_file(self):
        with tempfile.TemporaryDirectory() as directory:
            s = spec()
            ref.write_rows(directory, 'tkrzw', ref.module_of(s), {'b': dict(case='b')})
            ref.write_rows(directory, 'tkrzw', ref.module_of(s), {'a': dict(case='a'), 'b': dict(case='b', n=1)})
            text = Path(ref.reference_path(directory, 'tkrzw', ref.module_of(s))).read_text().splitlines()
            self.assertEqual([json.loads(line)['case'] for line in text], ['a', 'b'])
            with self.assertRaisesRegex(ref.ReferenceError, 'demo: no reference in'):
                ref.lookup(directory, 'tkrzw', s, None, NO_ABORT)

    def test_a_corrupt_file_is_a_reference_error_not_a_crash(self):
        with tempfile.TemporaryDirectory() as directory:
            path = ref.reference_path(directory, 'tkrzw', 'm')
            path.parent.mkdir(parents=True)
            path.write_text('not json\n')
            with self.assertRaisesRegex(ref.ReferenceError, 'm.jsonl:1'):
                ref.load_rows(directory, 'tkrzw', 'm')

    def test_kc_and_bdb_run_only_the_cases_the_tkrzw_references_call_data_free(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(ref.data_free_cases(directory, ['m']), set())
            rows = {name: dict(case=name, identity=dict(system_data=dict(kind=kind)))
                    for name, kind in (('a', ref.PACKAGE), ('b', ref.SYNTHETIC))}
            ref.write_rows(directory, 'tkrzw', 'm', rows)
            self.assertEqual(ref.data_free_cases(directory, ['m']), {'b'})

    def test_store_reads_a_module_once(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            s = spec()
            ref.write_rows(directory, 'tkrzw', ref.module_of(s), {'demo': row(s)})
            store = ref.Store(directory, 'tkrzw', None, NO_ABORT)
            with patch.object(ref, 'load_rows', wraps=ref.load_rows) as load:
                self.assertEqual(store.row(s), store.row(s))
                self.assertEqual(load.call_count, 1)


class ProfileTests(unittest.TestCase):
    def test_a_handoff_profile_round_trips_and_stays_small(self):
        with tempfile.TemporaryDirectory() as source, tempfile.TemporaryDirectory() as target:
            Path(source, 'user_bigram.db').write_bytes(b'\0' * 4_000_000 + b'tail')
            encoded = ref.encode_profile(source)
            self.assertLess(len(encoded['user_bigram.db']), 10_000)
            ref.decode_profile(encoded, target)
            self.assertEqual(Path(target, 'user_bigram.db').read_bytes(), b'\0' * 4_000_000 + b'tail')

    def test_decode_rejects_unsafe_name_and_bad_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            for name in ('../escape', '/absolute', 'dir/file', 'dir\\file', 'a..b'):
                with self.subTest(name=name), self.assertRaises(ref.ReferenceError):
                    ref.decode_profile({name: 'eJwDAAAAAAE='}, directory)
            for payload in ('!', 'YWJj'):
                with self.subTest(payload=payload), self.assertRaises(ref.ReferenceError):
                    ref.decode_profile({'profile.db': payload}, directory)


class CaptureEnvironmentTests(unittest.TestCase):
    def test_capture_refuses_a_host_that_is_not_the_stable_pin_image(self):
        release = Path('/etc/os-release')
        if release.is_file() and 'VERSION_CODENAME=trixie' in release.read_text():
            self.skipTest('this host is the pin image')
        with self.assertRaisesRegex(ref.ReferenceError, 'trixie'):
            ref.require_capture_environment()


if __name__ == '__main__':
    unittest.main()
