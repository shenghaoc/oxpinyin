#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Native startup refusal, supervision and statistical-contract tests."""
import contextlib
import io
import json
import os
import subprocess
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev import cli, slot, test_support
from tools.dev.measure import startup as st, worker, native, stats


class ControllerTests(unittest.TestCase):
    def test_optimised_controller_refuses_before_inputs_or_admission(self):
        for flags, value in (([], '1'), (['-O'], '0'), (['-OO'], '0')):
            with self.subTest(flags=flags, value=value), tempfile.TemporaryDirectory() as locks:
                result = subprocess.run(
                    [sys.executable, '-X', 'lazy_imports=normal', *flags,
                     '-m', 'tools.dev', 'measure', 'startup'], cwd=ROOT,
                    env=dict(os.environ, PYTHONOPTIMIZE=value, OXPINYIN_LOCK_DIR=locks),
                    capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(result.stdout, '')
                self.assertIn('startup: optimised Python is unsupported', result.stderr)
                self.assertIn('unset PYTHONOPTIMIZE and remove -O/-OO', result.stderr)
                self.assertNotIn('Traceback', result.stderr)
                self.assertEqual(list(Path(locks).iterdir()), [])

    def setUp(self):
        self.temp = self.enterContext(tempfile.TemporaryDirectory())
        self.root = Path(self.temp)
        for name in ('base', 'pr', 'data', 'profiles'):
            (self.root / name).mkdir()
        for name in st.PROFILES:
            root = self.root / 'profiles' / name
            (root / 'user').mkdir(parents=True)
            (root / 'profile.manifest').write_text(st.PROFILE.manifest(root / 'user'))
            (root / 'witness.txt').write_text('nihao\n' if name != 'empty' else '')
        self.argv = ['--base', str(self.root / 'base'), '--pr', str(self.root / 'pr'),
                     '--system-dir', str(self.root / 'data'), '--profiles-dir', str(self.root / 'profiles')]

    def test_pin_row_is_refused_before_inputs_or_slot_with_container_path(self):
        with mock.patch.object(slot, 'HeavyAdmission') as admission, contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(st.main(['--pin', 'some-pin']), 1)
        test_support.assert_not_called(self, admission)
        self.assertIn('just container run', err.getvalue())

    def test_populated_profiles_require_nonempty_witness(self):
        for profile in st.PROFILES[1:]:
            path = self.root / 'profiles' / profile / 'witness.txt'
            for raw in (None, '', ' \n'):
                if raw is None:
                    path.unlink(missing_ok=True)
                else:
                    path.write_text(raw)
                with self.subTest(profile=profile, raw=raw), contextlib.redirect_stderr(io.StringIO()) as err:
                    self.assertEqual(st.main([*self.argv, '--profiles', profile]), 1)
                self.assertIn('nonempty witness', err.getvalue())

    def test_wrong_manifest_including_historical_four_fields_is_refused(self):
        root = self.root / 'profiles/empty'
        (root / 'user/x').write_bytes(b'x')
        valid = st.PROFILE.manifest(root / 'user')
        for manifest in ('wrong\n', valid.replace('\n1\t', '\nfile\t1\t')):
            (root / 'profile.manifest').write_text(manifest)
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(st.main([*self.argv, '--profiles', 'empty']), 1)

    def test_cold_is_refused_on_windows_before_filesystem_or_slot(self):
        # Cold mode exists on Linux and macOS only; any other host refuses before admission.
        for system, extra in (('win32', []), ('win32', ['--cache', 'cold']), ('freebsd14', ['--cache', 'cold'])):
            message = 'startup: native startup supports Linux/macOS; Windows is deferred (#950)\n'
            with self.subTest(system=system), mock.patch.object(st.sys, 'platform', system), \
                    mock.patch.object(slot, 'HeavyAdmission') as admission, \
                    contextlib.redirect_stderr(io.StringIO()) as error:
                self.assertEqual(st.main([*self.argv, *extra]), 1)
            test_support.assert_not_called(self, admission)
            self.assertEqual(error.getvalue(), message)

    def test_cold_validates_on_macos_and_linux_with_no_approval_gate(self):
        args = st.parser().parse_args([*self.argv, '--profiles', 'empty', '--cache', 'cold'])
        for system in ('darwin', 'linux'):
            with self.subTest(system=system), mock.patch.object(st.sys, 'platform', system):
                self.assertEqual(st.validate(args), {'empty': self.root / 'profiles/empty/user'})

    def test_cold_refuses_a_work_directory_that_is_not_ext4_on_linux(self):
        args = st.parser().parse_args([*self.argv, '--profiles', 'empty', '--cache', 'cold'])
        with mock.patch.object(st.sys, 'platform', 'linux'), \
                mock.patch.object(st.native, 'filesystem', return_value=('tmpfs', '0:1')), \
                mock.patch.object(st.native, 'pin_cpu', return_value=None), \
                mock.patch.object(slot, 'HeavyAdmission') as admission, \
                self.assertRaisesRegex(ValueError, r'requires ext4, found tmpfs \(tmpfs is unsupported\)$'):
            st.run(args)
        test_support.assert_not_called(self, admission)

    def test_all_statuses(self):
        for result, expected in ((0, 0), (ValueError('refused'), 1), (st.NotResult('INVALID'), 3),
                                 (slot.Cancelled('cancelled'), 130), (KeyboardInterrupt(), 130)):
            with self.subTest(expected=expected), mock.patch.object(st, 'run') as run, \
                    contextlib.redirect_stderr(io.StringIO()):
                if isinstance(result, BaseException):
                    run.side_effect = result
                else:
                    run.return_value = result
                self.assertEqual(st.main([]), expected)
        for argv in (['--unknown'], ['--rounds', '0'], ['--rounds', 'n'], ['--warmups', '-1']):
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as exc:
                st.main(argv)
            self.assertEqual(exc.exception.code, 2)

    def test_native_command_timeouts_are_bounded_and_status_one(self):
        library = self.root / 'base/lib/libpinyin.dylib'
        library.parent.mkdir()
        library.write_bytes(b'oxpinyin')
        # Exercise every metadata command, including both otool options.
        calls = [
            ('darwin', native.subprocess, 'check_output', lambda: native.filesystem(self.root), ['df']),
            ('darwin', native.subprocess, 'run', lambda: native.filesystem(self.root), ['diskutil']),
            ('darwin', st.subprocess, 'check_output', lambda: st.library(self.root / 'base', 'pinyin'), ['nm']),
            ('darwin', st.subprocess, 'check_output', lambda: st.artifact(library), ['otool', '-L']),
            ('darwin', st.subprocess, 'check_output', lambda: st.artifact(library), ['otool', '-D']),
            ('linux', st.subprocess, 'check_output', lambda: st.artifact(library), ['readelf']),
        ]
        output = 'Filesystem blocks Used Available Capacity Mounted on\n/dev/disk3s1 100 20 80 20% /\n'
        for platform, module, method, operation, command in calls:
            def bounded(argv, **kwargs):
                if argv[:len(command)] != command:
                    return output
                self.assertEqual(kwargs['timeout'], native.COMMAND_TIMEOUT_SECONDS)
                raise subprocess.TimeoutExpired(argv, kwargs['timeout'])
            with self.subTest(command=command), mock.patch.object(st.sys, 'platform', platform), \
                    mock.patch.object(module, method, side_effect=bounded), \
                    mock.patch.object(st, 'run', side_effect=lambda args: operation()), \
                    contextlib.redirect_stderr(io.StringIO()), contextlib.ExitStack() as stack:
                if method == 'run':
                    stack.enter_context(mock.patch.object(module, 'check_output', return_value=output))
                self.assertEqual(st.main([]), 1)

    def test_work_parent_is_nonrecursive_but_inputs_stay_recursive(self):
        # An unrelated broken link under TMPDIR must not refuse the campaign. The
        # recursion rule is host-neutral; select Linux so the mocked ext4 kind is the
        # expected one on every host.
        (self.root / 'unrelated').symlink_to(self.root / 'missing')
        with mock.patch.object(native.sys, 'platform', 'linux'), \
                mock.patch.object(native, 'filesystem', return_value=('ext4', 'A')) as filesystem:
            self.assertEqual(native.check_filesystems([self.root], recursive=False), {'A'})
            filesystem.assert_called_once_with(self.root)
            with self.assertRaises(FileNotFoundError):
                native.check_filesystems([self.root])
        args = st.parser().parse_args([*self.argv, '--profiles', 'empty'])
        with mock.patch.object(st.tempfile, 'gettempdir', return_value=str(self.root)), \
                mock.patch.object(native, 'pin_cpu', return_value=None), \
                mock.patch.object(native, 'check_filesystems', return_value={'A'}) as check, \
                mock.patch.object(slot, 'HeavyAdmission'), mock.patch.object(st, 'campaign_run', return_value=0):
            self.assertEqual(st.run(args), 0)
        self.assertEqual(check.call_args_list, [
            mock.call([self.root], recursive=False),
            mock.call([args.system_dir, args.profiles_dir, args.base, args.pr])])

    def test_work_parent_and_inputs_still_require_one_apfs_volume(self):
        with mock.patch.object(st.sys, 'platform', 'darwin'), \
                mock.patch.object(native, 'pin_cpu', return_value=None), \
                mock.patch.object(native, 'check_filesystems', side_effect=[{'A'}, {'B'}]), \
                mock.patch.object(slot, 'HeavyAdmission') as admission, \
                contextlib.redirect_stderr(io.StringIO()) as error:
            self.assertEqual(st.main([*self.argv, '--profiles', 'empty']), 1)
        test_support.assert_not_called(self, admission)
        self.assertIn('one APFS volume', error.getvalue())

    def test_a_failing_admission_assertion_does_not_print_the_environment(self):
        # HeavyAdmission receives dict(os.environ): the shared helper must fail without printing it.
        sentinel = 'environment-sentinel-5f2c'
        args = st.parser().parse_args([*self.argv, '--profiles', 'empty'])
        with mock.patch.dict(os.environ, OXPINYIN_TEST_SENTINEL=sentinel), \
                mock.patch.object(st.tempfile, 'gettempdir', return_value=str(self.root)), \
                mock.patch.object(native, 'pin_cpu', return_value=None), \
                mock.patch.object(native, 'check_filesystems', return_value={'A'}), \
                mock.patch.object(slot, 'HeavyAdmission') as admission, \
                mock.patch.object(st, 'campaign_run', return_value=0):
            self.assertEqual(st.run(args), 0)
        self.assertEqual(admission.call_count, 1)
        self.assertEqual(admission.call_args.args[1]['OXPINYIN_TEST_SENTINEL'], sentinel)
        with self.assertRaises(AssertionError) as raised:
            test_support.assert_not_called(self, admission)
        # Not assertNotIn(sentinel, message): a failure would print the message, and with it the environment.
        self.assertFalse(sentinel in str(raised.exception), 'the assertion message echoes the environment')

    def test_cli_and_rotation(self):
        with mock.patch.object(st, 'main', return_value=3) as run:
            self.assertEqual(cli.main(['measure', 'startup', '--base', 'space path', '--bind', 'lazy']), 3)
            run.assert_called_once_with(['--base', 'space path', '--bind', 'lazy'])
        self.assertEqual([st.order(i) for i in range(3)],
                         [('base', 'pr', 'control'), ('pr', 'control', 'base'), ('control', 'base', 'pr')])

    def test_digest_hashes_exact_record_and_detects_mutation(self):
        raw = b'{"startup":"units","stopped_at":"end","text":"answer"}\n'
        _, digest = worker.parse(raw)
        self.assertEqual(digest, worker.sha256(raw))
        _, altered = worker.parse(raw.replace(b'answer', b'changed'))
        with self.assertRaises(st.NotResult):
            st.require_digest(dict(digest=altered), dict(digest=digest))
        for invalid in (raw[:-1], raw + raw, raw.replace(b'end', b'init')):
            with self.assertRaises(ValueError):
                worker.parse(invalid)

    def test_stats_are_moved_not_duplicated(self):
        td = st.module(ROOT / 'tools/bisection/teardown.py', 'td_shared')
        for name in ('summary', 'paired_ratio', 'stable_residency', 'exact_control'):
            self.assertIs(getattr(td, name), getattr(stats, name))
        self.assertTrue(stats.stable_control([100] * 4, [105] * 4)['stable'])
        self.assertFalse(stats.stable_control([100] * 4, [106] * 4)['stable'])
        self.assertTrue(stats.stable_residency([0] * 4, [64] * 4)['stable'])
        self.assertFalse(stats.stable_residency([0] * 4, [65] * 4)['stable'])
        self.assertEqual(stats.exact_control(10, 11)['verdict'], 'INVALID')

    @unittest.skipUnless(sys.platform == 'linux', 'Linux Callgrind controller')
    def test_exact_instruction_mismatch_stops_at_first_pair(self):
        libs = {a: {'pinyin': self.root / (a + '.so')} for a in st.ARMS}
        for value in libs.values():
            value['pinyin'].write_bytes(b'same bytes')
        campaign = SimpleNamespace(run=mock.Mock(side_effect=[
            dict(digest='same', stderr=b'summary: 100\n'),
            dict(digest='same', stderr=b'summary: 101\n')]))
        with mock.patch.object(st, 'emit') as emit, self.assertRaisesRegex(st.NotResult, 'STOP: same-bytes init Ir'):
            st.instrument(campaign, libs, 'pinyin', 'empty', self.root,
                          {a: dict(digest='same') for a in st.ARMS})
        self.assertEqual(campaign.run.call_count, 2)
        self.assertEqual(emit.call_args.kwargs['verdict'], 'INVALID')
        self.assertNotIn('pr_base_ratio', emit.call_args.kwargs)

    def range_campaign(self, *, wide=False, outside=False, init_variation=False, init_unresolved=False):
        libraries = {a: {'pinyin': self.root / (a + '.so')} for a in st.ARMS}
        for paths in libraries.values():
            paths['pinyin'].write_bytes(b'identical')
        visits = {}
        def run(path, facade, user, *, scope):
            arm = path.stem
            index = visits.get((scope, arm), 0)
            visits[scope, arm] = index + 1
            delta = (200 if wide else 10) if index % 2 else 0
            total = 100000 + (delta if scope != 'init' else int(init_variation and arm == 'pr'))
            fn = 'dedup_ranked_by_text_extra' if outside else 'oxpinyin_engine::session::dedup_ranked_by_text::h0123456789abcdef'
            raw = (f'creator: callgrind-3.27.1\npid: {index + 1}\ncmd: /worker measure\n'
                   f'positions: line\nevents: Ir\nsummary: {total}\nob={path}\n'
                   f'fn={fn}\n1 {total - 100}\nfn=stable\n2 100\ntotals: {total}\n').encode()
            if init_unresolved and scope == 'init':
                raw = raw.replace(fn.encode(), b'???')
            return dict(digest='same', stderr=raw, pid=index + 1)
        campaign = SimpleNamespace(run=run, executable=Path('/worker'))
        records = []
        with mock.patch.object(st.sys, 'platform', 'linux'), \
                mock.patch.object(st, 'emit', side_effect=lambda kind, **row: records.append(row)):
            try:
                st.instrument(campaign, libraries, 'pinyin', 'empty', self.root,
                              {a: dict(digest='same') for a in st.ARMS})
            except st.NotResult:
                pass
        return records, visits

    def test_randomstate_range_pass_reports_every_original(self):
        records, visits = self.range_campaign()
        self.assertEqual([r['verdict'] for r in records], ['VALID', 'RANGE-QUALIFIED', 'RANGE-QUALIFIED'])
        for row in records[1:]:
            self.assertEqual(row['bounds']['pr'], [100000, 100010])
            self.assertEqual(row['base_minus_pr_bounds'], [-10, 10])
            for arm in st.ARMS:
                self.assertEqual(visits[row['scope'], arm], 5)
                self.assertEqual(len(row['counts'][arm]), 5)
                self.assertEqual(len(row['profiles'][arm]), 5)
                self.assertIn('totals:', row['profiles'][arm][0]['raw'])

    def test_randomstate_wide_allowlisted_range_qualifies_with_wider_bounds(self):
        records, _ = self.range_campaign(wide=True)
        self.assertTrue(records[-1]['overlap'])
        self.assertEqual(records[-1]['verdict'], 'RANGE-QUALIFIED')
        self.assertEqual(records[-1]['base_minus_pr_bounds'], [-200, 200])

    def test_recheck_retained_profiles_without_launching_workers(self):
        records, _ = self.range_campaign(wide=True)
        start = dict(startup='start', worker=dict(path='/worker'), libraries={arm: {
            'pinyin': dict(path=str(self.root / (arm + '.so')))} for arm in st.ARMS})
        path = self.root / 'retained.jsonl'
        path.write_text('\n'.join(json.dumps(r) for r in [start, *[
            dict(r, startup='instructions') for r in records]]) + '\n')
        with mock.patch.object(worker, 'execute', side_effect=AssertionError('must not launch')), \
                mock.patch.object(st, 'emit') as emit:
            self.assertEqual(st.main(['--recheck-instructions', str(path)]), 0)
        self.assertEqual(emit.call_args.kwargs['base_minus_pr_bounds'], [-200, 200])
        self.assertEqual(emit.call_args.kwargs['counts']['pr'], [100000, 100200, 100000, 100200, 100000])
        replay = st.recheck_instructions(path)
        self.assertEqual(replay['scopes']['pinyin', 'empty', 'init']['creator'], 'callgrind-3.27.1')
        campaign = SimpleNamespace(run=mock.Mock(side_effect=AssertionError('covered scope rerun')))
        with mock.patch.object(st.sys, 'platform', 'linux'), mock.patch.object(st, 'emit'):
            st.instrument(campaign, {}, 'pinyin', 'empty', self.root, {}, completed=replay['scopes'])
        campaign.run.assert_not_called()
        path.write_text(path.read_text() + json.dumps(dict(records[-1], startup='instructions')) + '\n')
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            st.recheck_instructions(path)

    def test_recheck_rejects_retained_init_samples_from_different_profilers(self):
        records, _ = self.range_campaign()
        init = next(r for r in records if r['scope'] == 'init')
        original = bytes.fromhex(init['raw_profiles_hex']['pr'][1])
        changed = original.replace(b'creator: callgrind-3.27.1', b'creator: callgrind-3.26.0')
        self.assertNotEqual(changed, original)
        init['raw_profiles_hex']['pr'][1] = changed.hex()
        start = dict(startup='start', worker=dict(path='/worker'), libraries={arm: {
            'pinyin': dict(path=str(self.root / (arm + '.so')))} for arm in st.ARMS})
        path = self.root / 'retained.jsonl'
        path.write_text('\n'.join(json.dumps(r) for r in [start, *[
            dict(r, startup='instructions') for r in records]]) + '\n')
        # Equal totals pass every exact check; only the profiler identity differs,
        # which the live instrument path records as INVALID.
        with self.assertRaisesRegex(st.NotResult, 'retained exact profiler identity'):
            st.recheck_instructions(path)
        with mock.patch.object(worker, 'execute', side_effect=AssertionError('must not launch')), \
                mock.patch.object(st, 'emit'), contextlib.redirect_stderr(io.StringIO()) as error:
            self.assertEqual(st.main(['--recheck-instructions', str(path)]), 3)
        self.assertIn('INVALID: retained exact profiler identity', error.getvalue())

    def test_randomstate_outside_allowlist_is_invalid(self):
        records, _ = self.range_campaign(outside=True)
        self.assertEqual(records[-1]['verdict'], 'INVALID')
        self.assertEqual(records[-1]['outside_allowlist'][0]['function'], 'dedup_ranked_by_text_extra')

    def test_randomstate_init_variation_is_invalid(self):
        records, visits = self.range_campaign(init_variation=True)
        self.assertEqual(records[-1]['scope'], 'init')
        self.assertEqual(records[-1]['verdict'], 'INVALID')
        self.assertFalse(any(scope != 'init' for scope, arm in visits))

    def test_callgrind_identity_resolution_and_exclusive_accounting(self):
        raw = ('creator: callgrind-3.27.1\npid: 123\ncmd: /worker measure\npositions: line\n'
               'events: Ir\nsummary: 100\nob=/lib.so\nfn=(1) stable\n1 100\n'
               'cfn=(1)\ncalls=1 1\n1 100\ntotals: 100\n')
        parse = lambda text: st.instructions.profile(text.encode(), Path('/lib.so'), Path('/worker'), 123)
        self.assertEqual(parse(raw)['functions'], [dict(object='SUBJECT', function='stable', Ir=100)])
        for invalid in (raw.replace('creator:', 'other:'), raw + 'pid: 999\n', raw.replace('/worker', '/impostor'),
                        raw.replace('(1) stable', '(2)'), raw.replace('stable', '???'), raw.replace('pid: 123', 'pid: 456'),
                        raw.replace('summary: 100', 'summary: 99'), raw.replace('1 100', '1 99', 1)):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                parse(invalid)
        for fn in ('prefix_dedup_ranked_by_text', 'dedup_ranked_by_text_extra',
                   'other::dedup_ranked_by_text', 'order_window_tail [clone .isra.0]'):
            self.assertFalse(st.instructions.allowed(fn))

    def test_callgrind_header_whitespace_preserves_identity_checks(self):
        # Valgrind 3.27.1's reported cmd header has two spaces after the colon.
        raw = ('creator: callgrind-3.27.1\npid: 123\ncmd:  /worker measure pinyin\n'
               'positions: line\nevents: Ir\nsummary: 6922344\nob=/lib.so\n'
               'fn=stable\n1 6922344\ntotals: 6922344\n')
        parse = lambda text: st.instructions.profile(text.encode(), Path('/lib.so'), Path('/worker'), 123)
        for whitespace in (' ', '  ', '    ', '\t', ' \t  '):
            spaced = raw
            for field in ('creator', 'pid', 'cmd', 'events'):
                lines = spaced.splitlines(keepends=True)
                spaced = ''.join(field + ':' + whitespace + line.split(':', 1)[1].lstrip(' \t')
                                 if line.startswith(field + ':') else line for line in lines)
            with self.subTest(whitespace=whitespace):
                result = parse(spaced)
                self.assertEqual(result['total'], 6922344)
                self.assertEqual(result['creator'], 'callgrind-3.27.1')
                self.assertEqual(result['raw'], spaced)
                for invalid in (spaced.replace('/worker', '/impostor'),
                                spaced.replace('/worker measure', '/worker-extra measure'),
                                spaced.replace('123', '456'), spaced.replace('Ir\n', 'Ir Dr\n'),
                                spaced + 'creator: callgrind-other\n', spaced + 'pid: 123\n',
                                spaced + 'cmd:  /worker measure\n', spaced + 'events: Ir\n',
                                spaced.replace('/worker measure', '/workermeasure')):
                    with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                        parse(invalid)

    def test_exact_init_with_unresolved_entries_passes(self):
        records, _ = self.range_campaign(init_unresolved=True)
        self.assertEqual(records[0]['verdict'], 'VALID')
        self.assertEqual(records[0]['scope'], 'init')

    def unresolved_samples(self):
        profiles = {arm: [] for arm in st.ARMS}
        for arm in st.ARMS:
            for i in range(5):
                base = 0x100000 * (i + 1)
                path = '/lib.so'
                raw = (f'creator: callgrind-3.27.1\npid: 123\ncmd:  /worker measure\n'
                       f'positions: instr line\nevents: Ir\nsummary: 100000\nob={path}\n'
                       f'fn=???\n0x10 1 10\nfn=stable\n0x20 2 99990\ntotals: 100000\n')
                docs = [dict(startup='loaded_object', pid=123, object=path, load_bias=base,
                             segments=[dict(start=base, end=base + 4096, flags=5)])]
                profiles[arm].append(st.instructions.profile(raw.encode(), Path(path), Path('/worker'), 123, docs))
        return profiles

    def test_constant_unresolved_range_entry_passes(self):
        profiles = self.unresolved_samples()
        result = st.instructions.qualify({a: [100000] * 5 for a in st.ARMS}, profiles)
        self.assertEqual(result['verdict'], 'RANGE-QUALIFIED')
        self.assertEqual((result['constant_unresolved_count'], result['constant_unresolved_Ir']), (1, 10))
        self.assertEqual(profiles['pr'][0]['functions'][1]['function'], '@offset:0x10')

    def test_varying_unresolved_range_entry_is_invalid(self):
        profiles = self.unresolved_samples()
        profiles['control'][0]['functions'][1]['Ir'] += 1
        self.assertEqual(st.instructions.qualify({a: [100000] * 5 for a in st.ARMS}, profiles)['verdict'], 'INVALID')

    def test_unmatched_unresolved_range_entry_is_invalid(self):
        profiles = self.unresolved_samples()
        profiles['control'][0]['functions'].pop(1)
        self.assertEqual(st.instructions.qualify({a: [100000] * 5 for a in st.ARMS}, profiles)['verdict'], 'INVALID')

    def test_unresolved_mapping_identity_refuses_missing_wrong_or_ambiguous_loader(self):
        sample = self.unresolved_samples()['pr'][0]
        docs = sample['loader_data']
        for bad in ([], [dict(docs[0], pid=456)], docs + docs,
                    [dict(docs[0], object='/other.so')], [dict(docs[0], segments=[])]):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                st.instructions.profile(sample['raw'].encode(), Path('/lib.so'), Path('/worker'), 123, bad)
        for bad in (sample['raw'].replace('ob=/lib.so', 'ob=???'),
                    sample['raw'].replace('0x10 1 10', '0x100010 1 10'),
                    sample['raw'].replace('0x10 1 10', '* 1 10')):
            with self.subTest(raw=bad), self.assertRaises(ValueError):
                st.instructions.profile(bad.encode(), Path('/lib.so'), Path('/worker'), 123, docs)

    def test_range_checks_all_functions_even_when_totals_cancel(self):
        counts = {a: [100000] * 5 for a in st.ARMS}
        profiles = {a: [dict(creator='callgrind-test', functions=[
            dict(object='SUBJECT', function='dedup_ranked_by_text', Ir=100 + i),
            dict(object='SUBJECT', function='unrelated', Ir=99900 - i)]) for i in range(5)] for a in st.ARMS}
        result = st.instructions.qualify(counts, profiles)
        self.assertEqual(result['verdict'], 'INVALID')
        self.assertEqual(result['outside_allowlist'][0]['function'], 'unrelated')
        for samples in profiles.values():
            for sample in samples:
                sample['functions'][1]['Ir'] = 99900
        counts['control'] = [100001] * 5
        self.assertFalse(st.instructions.qualify(counts, profiles)['overlap'])

    def test_campaign_interleaves_and_excludes_warmups_and_memory_times(self):
        library = self.root / 'lib.so'
        library.write_bytes(b'synthetic oxpinyin')
        work = self.root / 'work'
        work.mkdir()
        args = st.parser().parse_args([*self.argv, '--profiles', 'empty', '--facades', 'pinyin',
                                      '--rounds', '2', '--warmups', '1', '--memory-rounds', '2', '--memory-warmups', '1'])
        args.cpu = None
        seen, printed = [], []
        def run(path, facade, profile, *, memory=False, cold=False):
            seen.append((path.parent.name, memory))
            self.assertFalse(cold)
            times = {metric: dict(wall_ns=999999 if memory else 100000, cpu_ns=1)
                     for metric in st.TIMING_METRICS if metric != 'process_wall'}
            points = {point: {metric: 100 for metric in native.MEMORY_FIELDS} for point in worker.POINTS}
            return dict(digest='same', stderr=b'', userfiles={}, documents=[dict(startup='times', **times)],
                        checkpoints=points, process_wall=100000)
        fake = SimpleNamespace(run=run)
        checks = mock.Mock()
        with mock.patch.object(st.native, 'check_filesystems'), mock.patch.object(st, 'library', return_value=library), \
                mock.patch.object(st.runpy, 'run_path', return_value={'check_arms': checks}), \
                mock.patch.object(worker, 'build', return_value=(Path('worker'), {})), \
                mock.patch.object(st, 'artifact', return_value={}), mock.patch.object(worker, 'Worker', return_value=fake), \
                mock.patch.object(st, 'verify', return_value={a: dict(digest='same', unit_stderr=b'', unit_userfiles={}) for a in st.ARMS}), \
                mock.patch.object(st, 'instrument'), mock.patch.object(st, 'emit', side_effect=lambda kind, **v: printed.append((kind, v))):
            status = st.campaign_run(args, {'empty': self.root / 'profiles/empty/user'}, work,
                                     SimpleNamespace(env={}, slot=SimpleNamespace(pass_fds=lambda: ())))
        self.assertEqual(status, 0)
        self.assertEqual(seen[:9], [(a, False) for a in ('baseobj', 'subject', 'control',
                                                       'subject', 'control', 'baseobj', 'control', 'baseobj', 'subject')])
        self.assertTrue(all(memory for _, memory in seen[9:]))
        self.assertEqual(len(seen), 18)
        orders = [v for k, v in printed if k == 'function-order']
        if sys.platform == 'linux':
            # #1062: every Linux shipping arm goes through the shared ELF ordering checker.
            self.assertEqual(set(checks.call_args.args[1]), set(st.ARMS))
            self.assertEqual(orders, [])
        else:
            # Mach-O has no function-order policy. The controller records the check as
            # unavailable once, claims no ordered arm and refuses none: all 18 runs above happened.
            checks.assert_not_called()
            self.assertEqual(orders, [dict(verdict='unavailable', reason='ELF policy is Linux-only')])
            self.assertFalse(any('ordered' in v.values() for _, v in printed))
        times = [v for k, v in printed if k == 'timing']
        self.assertEqual(len(times), len(st.TIMING_METRICS))
        self.assertTrue(all(v['summaries']['pr']['n'] == 2 and v['summaries']['pr']['median'] == .1 for v in times))
        self.assertTrue(all(v['pr_base_ratio']['median'] == 1 for v in times))

    def test_campaign_passes_cold_to_timed_and_memory_samples_and_records_the_metric(self):
        library = self.root / 'lib.so'
        library.write_bytes(b'synthetic oxpinyin')
        work = self.root / 'work'
        work.mkdir()
        args = st.parser().parse_args([*self.argv, '--profiles', 'empty', '--facades', 'pinyin',
                                      '--rounds', '1', '--warmups', '0', '--memory-rounds', '1',
                                      '--memory-warmups', '0', '--cache', 'cold'])
        args.cpu = None
        calls, printed = [], []

        def run(path, facade, profile, *, memory=False, cold=False):
            calls.append(cold)
            times = {metric: dict(wall_ns=100000, cpu_ns=1)
                     for metric in st.TIMING_METRICS if metric != 'process_wall'}
            points = {point: {metric: 100 for metric in native.MEMORY_FIELDS} for point in worker.POINTS}
            cold = dict(metric='darwin_resident_pages', value=0, resident=0, pages=1,
                        attempts=1, verdict='QUALIFIED', checks=[], files=[]) if cold else None
            return dict(digest='same', stderr=b'', userfiles={}, documents=[dict(startup='times', **times)],
                        checkpoints=points, process_wall=100000, cold=cold)

        fake = SimpleNamespace(run=run)
        with mock.patch.object(st.sys, 'platform', 'darwin'), \
                mock.patch.object(st.native, 'check_filesystems'), \
                mock.patch.object(st, 'library', return_value=library), \
                mock.patch.object(worker, 'build', return_value=(Path('worker'), {})), \
                mock.patch.object(st, 'artifact', return_value={}), \
                mock.patch.object(worker, 'Worker', return_value=fake), \
                mock.patch.object(st, 'verify', return_value={a: dict(digest='same', unit_stderr=b'',
                                                                       unit_userfiles={}) for a in st.ARMS}), \
                mock.patch.object(st, 'instrument'), \
                mock.patch.object(st, 'emit', side_effect=lambda kind, **v: printed.append((kind, v))):
            status = st.campaign_run(args, {'empty': self.root / 'profiles/empty/user'}, work,
                                     SimpleNamespace(env={}, slot=SimpleNamespace(pass_fds=lambda: ())))
        self.assertEqual(status, 0)
        self.assertTrue(calls)
        self.assertTrue(all(calls))
        samples = [v for kind, v in printed if kind == 'sample']
        self.assertEqual(len(samples), 6)
        self.assertTrue(all(v['cold']['metric'] == 'darwin_resident_pages' for v in samples))

    def synthetic_campaign(self, *options, platform='darwin', rejections=None):
        """Run campaign_run over a scripted worker and return its status and printed records.

        Rows are deterministic functions of (arm, memory, round), shaped like macOS rows: the
        Darwin names carry values and the Linux names are unavailable. With a cold cache every
        sample runs the host's real preparation (cold.prepare on macOS, cold_linux.prepare on
        Linux) over fake eviction and residency readers. REJECTIONS maps
        (arm, memory, round) to how many of that sample's whole-set checks find a page
        resident; two reject the sample, one qualifies it on the second preparation.
        """
        rejections = rejections or {}
        directories = dict(zip(('baseobj', 'subject', 'control'), st.ARMS))
        visits, resident = {}, [0]
        subject = self.root / 'subject.bin'
        subject.write_bytes(b'x' * (3 * st.cold.darwin.PAGE_SIZE))

        def residency(_fd):
            """Fake mincore: the next checks find two of three pages resident, then none."""
            if resident[0]:
                resident[0] -= 1
                return dict(pages=3, resident=2)
            return dict(pages=3, resident=0)

        def run(path, facade, profile, *, memory=False, cold=False):
            arm = directories[path.parent.name]
            n = visits[arm, memory] = visits.get((arm, memory), -1) + 1
            prepared = None
            if cold:
                resident[0] = rejections.get((arm, memory, n), 0)
                if platform == 'linux':
                    prepared = st.cold_linux.prepare([subject], self.root)
                else:
                    prepared = st.cold.prepare([subject])
            shift = 0 if arm == 'base' else 3
            times = {}
            for index, metric in enumerate(m for m in st.TIMING_METRICS if m != 'process_wall'):
                wall = 100_000 + 1_000 * n + 100 * shift + index
                times[metric] = dict(wall_ns=wall * 1.2 if (arm, metric) == ('control', 'fini') else wall, cpu_ns=1)
            points = {}
            for index, point in enumerate(worker.POINTS):
                resident_kib = 10_000 + 7 * n + index + (200 if arm == 'control' else 0)
                points[point] = dict.fromkeys(native.MEMORY_FIELDS) | dict(
                    resident_kib=resident_kib, resident_peak_kib=resident_kib + 100,
                    physical_footprint_kib=20_000 + n + index, faults=50 + n, pageins=n,
                    disk_bytes_read=4096 * n, disk_bytes_written=0, instructions=1_000 + n,
                    cycles=2_000 + n, instructions_available=True, cycles_available=True)
            return dict(digest='same', stderr=b'', userfiles={}, documents=[dict(startup='times', **times)],
                        checkpoints=points, process_wall=1_000_000 + 10_000 * n + shift, cold=prepared)

        library = self.root / 'lib.so'
        library.write_bytes(b'synthetic oxpinyin')
        work = Path(tempfile.mkdtemp(dir=self.root))
        args = st.parser().parse_args([*self.argv, '--profiles', 'empty', '--facades', 'pinyin', *options])
        args.cpu = None
        printed = []
        with mock.patch.object(st.sys, 'platform', platform), \
                mock.patch.object(st.native, 'check_filesystems'), \
                mock.patch.object(st, 'library', return_value=library), \
                mock.patch.object(worker, 'build', return_value=(Path('worker'), {})), \
                mock.patch.object(st, 'artifact', return_value={}), \
                mock.patch.object(worker, 'Worker', return_value=SimpleNamespace(run=run)), \
                mock.patch.object(st, 'verify', return_value={a: dict(digest='same', unit_stderr=b'',
                                                                       unit_userfiles={}) for a in st.ARMS}), \
                mock.patch.object(st, 'instrument'), \
                mock.patch.object(st, 'emit', side_effect=lambda kind, **v: printed.append((kind, v))), \
                mock.patch.object(st.cold, '_touch'), \
                mock.patch.object(st.cold.darwin, 'map_file', return_value=0x1000), \
                mock.patch.object(st.cold.darwin, 'unmap'), mock.patch.object(st.cold.darwin, 'msync'), \
                mock.patch.object(st.cold.darwin, 'file_residency', side_effect=residency), \
                mock.patch.object(st.cold_linux.mman, 'residency', side_effect=residency), \
                mock.patch.object(st.cold_linux, 'evict'), \
                mock.patch.object(st.cold_linux.native, 'filesystem', return_value=('ext4', '0:1')), \
                mock.patch.object(st.runpy, 'run_path', return_value=dict(check_arms=lambda *_: None)), \
                mock.patch.object(st.os, 'sched_getaffinity', return_value={0}, create=True), \
                mock.patch.object(st.os, 'sched_setaffinity', create=True), \
                mock.patch.object(st.cold.os, 'fsync'):
            status = st.campaign_run(args, {'empty': self.root / 'profiles/empty/user'}, work,
                                     SimpleNamespace(env={}, slot=SimpleNamespace(pass_fds=lambda: ())))
        return status, printed

    def test_macos_warm_campaign_output_is_byte_identical_to_before(self):
        # The digest is the SHA-256 of the sample, timing and memory records this scripted warm macOS
        # campaign printed before cold sampling was enabled: STABLE, INCONCLUSIVE, diagnostic and
        # unavailable verdicts all appear, and a warm record carries no cold key.
        status, printed = self.synthetic_campaign('--rounds', '5', '--warmups', '1',
                                                  '--memory-rounds', '5', '--memory-warmups', '1')
        self.assertEqual(status, 0)
        records = [[kind, values] for kind, values in printed if kind in ('sample', 'timing', 'memory')]
        fields = (len(native.MEMORY_FIELDS) + len(native.DARWIN_COUNTER_FIELDS) +
                  len(native.DARWIN_AVAILABILITY_FIELDS))
        self.assertEqual(len(records), 36 + len(st.TIMING_METRICS) + len(worker.POINTS) * fields)
        verdicts = {values['verdict'] for kind, values in records if kind != 'sample'}
        self.assertEqual(verdicts, {'STABLE', 'INCONCLUSIVE', 'diagnostic', 'unavailable'})
        self.assertFalse(any('cold' in values for _, values in records))
        digest = worker.sha256(json.dumps(records, sort_keys=True).encode())
        self.assertEqual(digest, 'cbac14bfb42027503822ee43092d1469d9705314d9371381682f5b34560cf4d2')

    def report(self, printed, kind, metric, point=None):
        """The one metric report of a kind (and checkpoint) the scripted campaign printed."""
        found = [v for k, v in printed if k == kind and v['metric'] == metric and v.get('point') == point]
        self.assertEqual(len(found), 1)
        return found[0]

    COLD = ('--cache', 'cold')

    def test_rejected_sample_is_reported_excluded_and_never_replaced(self):
        # PR's second measured timing round never qualifies: two preparations leave pages resident.
        status, printed = self.synthetic_campaign(
            *self.COLD, '--rounds', '4', '--warmups', '1', '--memory-rounds', '3', '--memory-warmups', '1',
            rejections={('pr', False, 2): 2})
        self.assertEqual(status, 0)
        samples = [v for k, v in printed if k == 'sample']
        # Round accounting is unchanged: every attempted round has its record and none was rerun.
        self.assertEqual(len(samples), (4 + 1) * 3 + (3 + 1) * 3)
        rejected = [s for s in samples if s['cold']['verdict'] == 'REJECTED']
        self.assertEqual(len(rejected), 1)
        self.assertEqual((rejected[0]['arm'], rejected[0]['round'], rejected[0]['warmup'], rejected[0]['memory']),
                         ('pr', 2, False, False))
        self.assertEqual((rejected[0]['cold']['metric'], rejected[0]['cold']['attempts'],
                          rejected[0]['cold']['resident']), ('darwin_resident_pages', 2, 2))
        self.assertTrue({'documents', 'checkpoints', 'process_wall_ns'}.isdisjoint(rejected[0]))
        self.assertTrue(all(s['cold']['verdict'] == 'QUALIFIED' and s['cold']['attempts'] == 1
                            for s in samples if s is not rejected[0]))
        timing = self.report(printed, 'timing', 'process_wall')
        self.assertEqual(timing['cold'], dict(
            metric='darwin_resident_pages', limit=2, accepted=dict(base=4, pr=3, control=4),
            rejected=dict(base=0, pr=1, control=0), warmup_rejected=dict(base=0, pr=0, control=0)))
        # Rounds 1, 3 and 4 pool for every arm: base's wall times there are 1.01, 1.03 and 1.04 ms.
        self.assertEqual({arm: s['n'] for arm, s in timing['summaries'].items()}, dict(base=3, pr=3, control=3))
        self.assertEqual(timing['summaries']['base']['median'], 1.03)
        self.assertEqual(timing['verdict'], 'STABLE')
        self.assertIsNotNone(timing['pr_base_ratio'])
        # The memory family had no rejection; a Linux-named metric stays unavailable but is still counted.
        memory = self.report(printed, 'memory', 'resident_kib', 'end')
        self.assertEqual((memory['cold']['rejected'], memory['summaries']['pr']['n']),
                         (dict(base=0, pr=0, control=0), 3))
        unavailable = self.report(printed, 'memory', 'rss', 'end')
        self.assertEqual((unavailable['verdict'], unavailable['summaries']), ('unavailable', None))
        self.assertEqual(unavailable['cold']['rejected'], dict(base=0, pr=0, control=0))

    def test_two_rejections_in_an_arm_keep_a_metric_eligible_and_three_make_it_inconclusive(self):
        for platform in ('darwin', 'linux'):
            with self.subTest(platform=platform):
                self.limit_at_two_and_three(platform)

    def limit_at_two_and_three(self, platform):
        """Two PR rejections stay eligible and three are INCONCLUSIVE, under the host's preparation."""
        options = (*self.COLD, '--rounds', '8', '--warmups', '1', '--memory-rounds', '2', '--memory-warmups', '0')
        status, printed = self.synthetic_campaign(*options, platform=platform,
                                                  rejections={('pr', False, 2): 2, ('pr', False, 4): 2})
        self.assertEqual(status, 0)
        eligible = self.report(printed, 'timing', 'process_wall')
        self.assertEqual((eligible['cold']['rejected']['pr'], eligible['verdict']), (2, 'STABLE'))
        self.assertEqual(eligible['summaries']['pr']['n'], 6)
        self.assertIsNotNone(eligible['pr_base_ratio'])
        self.assertFalse(any('reason' in v for k, v in printed if k == 'timing'))
        status, printed = self.synthetic_campaign(
            *options, platform=platform,
            rejections={('pr', False, 2): 2, ('pr', False, 4): 2, ('pr', False, 5): 2})
        self.assertEqual(status, 0)
        timing = [v for k, v in printed if k == 'timing']
        self.assertEqual({v['verdict'] for v in timing}, {'INCONCLUSIVE'})
        self.assertEqual(len(timing), len(st.TIMING_METRICS))
        for record in timing:
            self.assertEqual((record['summaries'], record['pr_base_ratio']), (None, None))
            self.assertEqual(record['reason'], 'rejected cold samples exceed the limit of 2 per arm: pr 3')
            self.assertEqual((record['cold']['accepted']['pr'], record['cold']['rejected']['pr']), (5, 3))
        # The limit is per arm and per metric family: the memory metrics saw no rejection.
        memory = 'physical_footprint_kib' if platform == 'darwin' else 'resident_kib'
        self.assertNotIn('reason', self.report(printed, 'memory', memory, 'end'))

    def test_the_limit_counts_each_arm_alone_and_pools_only_rounds_every_arm_accepted(self):
        options = (*self.COLD, '--rounds', '8', '--warmups', '1', '--memory-rounds', '2', '--memory-warmups', '0')
        # Two rejections in each of the three arms: six rejected samples, none over the limit.
        rejections = {(arm, False, n): 2 for arm, rounds in (('base', (1, 2)), ('pr', (3, 4)), ('control', (5, 6)))
                      for n in rounds}
        _, printed = self.synthetic_campaign(*options, rejections=rejections)
        timing = self.report(printed, 'timing', 'process_wall')
        self.assertEqual(timing['cold']['rejected'], dict(base=2, pr=2, control=2))
        self.assertEqual(timing['cold']['accepted'], dict(base=6, pr=6, control=6))
        self.assertEqual({arm: s['n'] for arm, s in timing['summaries'].items()}, dict(base=2, pr=2, control=2))
        self.assertEqual(timing['verdict'], 'STABLE')
        # One rejection per arm, each in its own round, leaves no round to compare.
        _, printed = self.synthetic_campaign(*self.COLD, '--rounds', '3', '--warmups', '0',
                                             '--memory-rounds', '2', '--memory-warmups', '0',
                                             rejections={('base', False, 0): 2, ('pr', False, 1): 2,
                                                         ('control', False, 2): 2})
        timing = self.report(printed, 'timing', 'process_wall')
        self.assertEqual((timing['cold']['rejected'], timing['verdict'], timing['summaries']),
                         (dict(base=1, pr=1, control=1), 'INCONCLUSIVE', None))
        self.assertEqual(timing['reason'], 'no measured round has every arm accepted')

    def test_rejected_warmups_are_reported_apart_and_never_count_toward_the_limit(self):
        _, printed = self.synthetic_campaign(
            *self.COLD, '--rounds', '3', '--warmups', '3', '--memory-rounds', '2', '--memory-warmups', '0',
            rejections={('pr', False, n): 2 for n in range(3)})
        timing = self.report(printed, 'timing', 'process_wall')
        self.assertEqual((timing['cold']['rejected']['pr'], timing['cold']['warmup_rejected']['pr']), (0, 3))
        self.assertEqual((timing['verdict'], timing['summaries']['pr']['n']), ('STABLE', 3))
        warmups = [v for k, v in printed if k == 'sample' and v['cold']['verdict'] == 'REJECTED']
        self.assertEqual([(v['arm'], v['round'], v['warmup']) for v in warmups],
                         [('pr', n, True) for n in range(3)])

    def test_a_set_that_qualifies_on_the_second_preparation_is_accepted(self):
        _, printed = self.synthetic_campaign(
            *self.COLD, '--rounds', '2', '--warmups', '0', '--memory-rounds', '2', '--memory-warmups', '0',
            rejections={('base', False, 1): 1})
        second = [v for k, v in printed if k == 'sample' and v['cold']['attempts'] == 2]
        self.assertEqual(len(second), 1)
        self.assertEqual((second[0]['cold']['verdict'], second[0]['cold']['value']), ('QUALIFIED', 0))
        self.assertEqual([c['resident'] for c in second[0]['cold']['checks']], [2, 0])
        self.assertEqual(self.report(printed, 'timing', 'process_wall')['cold']['rejected'],
                         dict(base=0, pr=0, control=0))

    def test_the_limit_is_the_shared_constant_whatever_the_results(self):
        options = (*self.COLD, '--rounds', '6', '--warmups', '1', '--memory-rounds', '2', '--memory-warmups', '0')
        two = {('pr', False, 2): 2, ('pr', False, 3): 2}
        many = {('pr', False, n): 2 for n in range(1, 7)}
        seen = set()
        for rejections in ({}, two, many):
            _, printed = self.synthetic_campaign(*options, rejections=rejections)
            seen |= {v['cold']['limit'] for k, v in printed if k in ('timing', 'memory')}
        self.assertEqual(seen, {2})
        self.assertEqual(self.report(printed, 'timing', 'process_wall')['verdict'], 'INCONCLUSIVE')
        # The same two rejections cross the boundary only when the shared constant moves.
        with mock.patch.object(st.cold, 'REJECTION_LIMIT', 1):
            _, printed = self.synthetic_campaign(*options, rejections=two)
        moved = self.report(printed, 'timing', 'process_wall')
        self.assertEqual((moved['verdict'], moved['cold']['limit']), ('INCONCLUSIVE', 1))
        self.assertEqual(st.cold.REJECTION_LIMIT, 2)

    def test_the_darwin_metric_stays_out_of_every_linux_named_field(self):
        options = ('--rounds', '3', '--warmups', '1', '--memory-rounds', '2', '--memory-warmups', '0')
        _, warm_run = self.synthetic_campaign(*options)
        _, cold_run = self.synthetic_campaign(*self.COLD, *options, rejections={('pr', False, 2): 2})

        def reported(printed):
            """Every (kind, metric, checkpoint) the campaign reported."""
            return {(k, v['metric'], v.get('point')) for k, v in printed if k in ('timing', 'memory')}

        self.assertEqual(reported(cold_run), reported(warm_run))
        self.assertNotIn('darwin_resident_pages', {metric for _, metric, _ in reported(cold_run)})
        for kind, record in cold_run:
            if kind in ('timing', 'memory'):
                self.assertTrue(set(native.LINUX_MEMORY_FIELDS).isdisjoint(record['cold']))
                self.assertEqual(record['cold']['metric'], 'darwin_resident_pages')
            if kind == 'sample':
                self.assertEqual(record['cold']['metric'], 'darwin_resident_pages')
                self.assertNotIn('darwin_resident_pages', json.dumps(record.get('checkpoints', {})))
        for _, record in warm_run:
            self.assertNotIn('cold', record)

    def test_linux_cold_run_rejects_like_macos_under_its_own_metric(self):
        options = (*self.COLD, '--rounds', '4', '--warmups', '1', '--memory-rounds', '2', '--memory-warmups', '0')
        status, printed = self.synthetic_campaign(*options, platform='linux',
                                                  rejections={('pr', False, 2): 2, ('base', False, 1): 1})
        self.assertEqual(status, 0)
        samples = [v for k, v in printed if k == 'sample']
        rejected = [s for s in samples if s['cold']['verdict'] == 'REJECTED']
        self.assertEqual([(s['arm'], s['round']) for s in rejected], [('pr', 2)])
        self.assertEqual((rejected[0]['cold']['attempts'], rejected[0]['cold']['resident']), (2, 2))
        # One check found a page resident, the second eviction cleared it: accepted.
        second = [s for s in samples if s['cold']['verdict'] == 'QUALIFIED' and s['cold']['attempts'] == 2]
        self.assertEqual([(s['arm'], s['round']) for s in second], [('base', 1)])
        timing = self.report(printed, 'timing', 'process_wall')
        self.assertEqual(timing['cold'], dict(
            metric='linux_resident_pages', limit=2, accepted=dict(base=4, pr=3, control=4),
            rejected=dict(base=0, pr=1, control=0), warmup_rejected=dict(base=0, pr=0, control=0)))
        self.assertEqual(timing['verdict'], 'STABLE')
        for kind, record in printed:
            if 'cold' in record:
                self.assertEqual(record['cold']['metric'], 'linux_resident_pages')
        self.assertNotIn('darwin_resident_pages', json.dumps(printed))

    def test_pool_cold_and_cold_account_are_plain_arithmetic_on_the_shared_limit(self):
        rejected = st.REJECTED
        samples = dict(base=[dict(), None, dict(), dict()], pr=[dict(), dict(), None, dict()], control=[dict()] * 4)
        for system, metric in (('linux', 'linux_resident_pages'), ('darwin', 'darwin_resident_pages')):
            with mock.patch.object(st.sys, 'platform', system):
                account = st.cold_account(samples, dict(base=0, pr=1, control=0))
            self.assertEqual(account, dict(metric=metric, limit=2,
                                           rejected=dict(base=1, pr=1, control=0),
                                           accepted=dict(base=3, pr=3, control=4),
                                           warmup_rejected=dict(base=0, pr=1, control=0)))
        values = dict(base=[1, rejected, 3, 4], pr=[1, 2, rejected, 4], control=[1, 2, 3, 4])
        self.assertEqual(st.pool_cold(values, account), (dict(base=[1, 4], pr=[1, 4], control=[1, 4]), None))
        over = dict(account, rejected=dict(base=3, pr=0, control=3))
        self.assertEqual(st.pool_cold(values, over),
                         (None, 'rejected cold samples exceed the limit of 2 per arm: base 3, control 3'))
        exact = dict(account, rejected=dict(base=2, pr=2, control=2))
        self.assertIsNone(st.pool_cold(values, exact)[1])
        self.assertEqual(st.pool_cold(dict(base=[rejected], pr=[1], control=[1]), account),
                         (None, 'no measured round has every arm accepted'))

    def cold_report(self, values):
        """Report one cold memory metric from per-round values (REJECTED where rejected); return its record."""
        samples = {arm: [None if value is st.REJECTED else {} for value in rows] for arm, rows in values.items()}
        account = st.cold_account(samples, dict.fromkeys(st.ARMS, 0))
        with mock.patch.object(st, 'emit') as emit:
            st.report_values('memory', values, facade='pinyin', profile='empty', point='end',
                             metric='resident_kib', unit='KiB', cold=account)
        self.assertEqual(emit.call_count, 1)
        return emit.call_args.kwargs

    def test_a_field_none_in_every_kept_sample_is_unavailable_even_over_the_limit(self):
        record = self.cold_report(dict(base=[None] * 4, pr=[st.REJECTED] * 3 + [None], control=[None] * 4))
        self.assertEqual(record['cold']['rejected']['pr'], 3)
        self.assertEqual((record['verdict'], record['summaries'], record['pr_base_ratio']), ('unavailable', None, None))
        self.assertNotIn('reason', record)

    def test_a_none_in_a_dropped_round_does_not_make_the_metric_unavailable(self):
        # The review's case: PR's round 2 is rejected, so round 2 leaves the pool with base's None in it.
        record = self.cold_report(dict(base=[10, None, 12], pr=[10, st.REJECTED, 12], control=[10, 10, 12]))
        self.assertEqual(record['verdict'], 'STABLE')
        self.assertEqual({arm: s['n'] for arm, s in record['summaries'].items()}, dict(base=2, pr=2, control=2))
        self.assertEqual(record['summaries']['base']['median'], 11)
        self.assertEqual(record['pr_base_ratio']['median'], 1.0)

    def test_a_none_in_a_kept_round_is_still_unavailable(self):
        record = self.cold_report(dict(base=[10, None, 12], pr=[st.REJECTED, 11, 12], control=[10, 10, 12]))
        self.assertEqual((record['verdict'], record['summaries'], record['pr_base_ratio']), ('unavailable', None, None))
        self.assertNotIn('reason', record)

    def test_recorded_values_over_the_limit_are_inconclusive_with_a_reason(self):
        record = self.cold_report(dict(base=[10, 11, 12, 13], pr=[st.REJECTED] * 3 + [13], control=[10, 11, 12, 13]))
        self.assertEqual((record['verdict'], record['summaries'], record['pr_base_ratio']), ('INCONCLUSIVE', None, None))
        self.assertEqual(record['reason'], 'rejected cold samples exceed the limit of 2 per arm: pr 3')

    def test_every_sample_rejected_is_inconclusive_not_unavailable(self):
        # Nothing was recorded, so nothing shows the host lacks the field: pool_cold decides.
        for rounds, reason in ((3, 'rejected cold samples exceed the limit of 2 per arm: base 3, pr 3, control 3'),
                               (2, 'no measured round has every arm accepted')):
            with self.subTest(rounds=rounds):
                record = self.cold_report({arm: [st.REJECTED] * rounds for arm in st.ARMS})
                self.assertEqual((record['verdict'], record['summaries'], record['pr_base_ratio']),
                                 ('INCONCLUSIVE', None, None))
                self.assertEqual(record['reason'], reason)

    def test_one_arm_fully_rejected_beside_recorded_values_is_inconclusive(self):
        record = self.cold_report(dict(base=[10, 11, 12], pr=[st.REJECTED] * 3, control=[10, 11, 12]))
        self.assertEqual((record['verdict'], record['summaries'], record['pr_base_ratio']), ('INCONCLUSIVE', None, None))
        self.assertEqual(record['reason'], 'rejected cold samples exceed the limit of 2 per arm: pr 3')

    def test_worker_rejected_cold_preparation_never_spawns_and_cleans_up(self):
        # The real cold.prepare runs over a fake residency reader, so the private user copy is part of the
        # rejected set; afterwards neither it nor its sample directory remains and the next sample runs.
        work = self.root / 'work'
        work.mkdir()
        data = self.root / 'data'
        (data / 'system.bin').write_bytes(b's')
        root = self.root / 'profile'
        profile = root / 'user'
        profile.mkdir(parents=True)
        (profile / 'user.bin').write_bytes(b'u')
        (root / 'profile.manifest').write_text(st.PROFILE.manifest(profile))
        library = self.root / 'libpinyin.dylib'
        library.write_bytes(b'lib')
        resident = [True]

        def check(handle, path):
            """Fake residency reader: one page per file, resident until the flag is cleared."""
            return dict(path=str(path), pages=1, resident=int(resident[0]))

        result = dict(status=0, pid=1, stdout=b'', stderr=b'', checkpoints={}, process_wall=1)
        with mock.patch.object(worker, 'check_identity'), \
                mock.patch.object(worker, 'execute', return_value=result) as execute, \
                mock.patch.object(worker, 'parse', return_value=([], 'digest')), \
                mock.patch.object(worker, 'userfiles', return_value={}), \
                mock.patch.object(worker, 'exports', return_value=[]), \
                mock.patch.object(st.cold, '_invalidate'), \
                mock.patch.object(st.cold, '_check', side_effect=check), \
                mock.patch.object(st.cold.os, 'fsync'), \
                mock.patch.object(worker.sys, 'platform', 'darwin'):
            campaign = worker.Worker(Path('worker'), {}, data, work, {}, (), None, 'now')
            with self.assertRaises(st.cold.Rejected) as raised:
                campaign.run(library, 'pinyin', profile, cold=True)
            test_support.assert_not_called(self, execute)
            record = raised.exception.record
            self.assertEqual((record['verdict'], record['attempts'], record['resident']), ('REJECTED', 2, 3))
            files = {Path(row['path']).name: Path(row['path']) for row in record['files']}
            self.assertEqual(set(files), {'libpinyin.dylib', 'system.bin', 'user.bin'})
            copy = files['user.bin']
            self.assertEqual(copy.parent.parent, work.resolve() / 'sample')
            self.assertFalse(copy.exists())
            self.assertFalse((work / 'sample').exists())
            self.assertEqual(list(work.iterdir()), [])
            self.assertTrue(files['libpinyin.dylib'].exists() and files['system.bin'].exists())
            # A stale sample directory would refuse this run.
            resident[0] = False
            row = campaign.run(library, 'pinyin', profile, cold=True)
            self.assertEqual((row['cold']['verdict'], row['cold']['attempts']), ('QUALIFIED', 1))
            self.assertEqual(execute.call_count, 1)
            self.assertEqual(list(work.iterdir()), [])

    def test_worker_prepares_cold_files_before_spawning(self):
        work = self.root / 'work'
        work.mkdir()
        data = self.root / 'data'
        (data / 'system.bin').write_bytes(b's')
        root = self.root / 'profile'
        profile = root / 'user'
        profile.mkdir(parents=True)
        (profile / 'user.bin').write_bytes(b'u')
        (root / 'profile.manifest').write_text(st.PROFILE.manifest(profile))
        library = self.root / 'libpinyin.dylib'
        library.write_bytes(b'lib')
        prepared = dict(metric='darwin_resident_pages', value=0, resident=0, pages=1,
                        attempts=1, verdict='QUALIFIED', checks=[], files=[])
        events = []

        def prepare(files):
            events.append(('prepare', [Path(path).name for path in files]))
            return prepared

        def execute(command, **_kwargs):
            events.append(('execute', command))
            return dict(status=0, pid=1, stdout=b'', stderr=b'', checkpoints={}, process_wall=1)

        with mock.patch.object(worker, 'check_identity'), \
                mock.patch.object(worker, 'execute', side_effect=execute), \
                mock.patch.object(worker, 'parse', return_value=([], 'digest')), \
                mock.patch.object(worker, 'userfiles', return_value={}), \
                mock.patch.object(worker, 'exports', return_value=[]), \
                mock.patch.object(worker.cold_cache, 'prepare', side_effect=prepare), \
                mock.patch.object(worker.sys, 'platform', 'darwin'):
            campaign = worker.Worker(Path('worker'), {}, data, work, {}, (), None, 'now')
            row = campaign.run(library, 'pinyin', profile, cold=True)
        self.assertEqual([event[0] for event in events], ['prepare', 'execute'])
        self.assertEqual(events[0][1], ['libpinyin.dylib', 'system.bin', 'user.bin'])
        self.assertEqual(row['cold'], prepared)

    def test_teardown_equivalence_blocked_is_status_three(self):
        td = st.module(ROOT / 'tools/bisection/teardown.py', 'td_status')
        def run(facade, library, **kwargs):
            return dict(observation=dict(samples=[str(library)], stderr='', userfiles={}), raw_profile_identity='x')
        campaign = SimpleNamespace(work=self.root, run=run)
        with mock.patch.object(td, 'prepare_witnesses', return_value={'empty': None}), mock.patch.object(td, 'print_record'):
            status = td.execute(campaign, SimpleNamespace(teardown_stage='verify'),
                                {a: {'pinyin': Path(a)} for a in ('ox', 'pin', 'control')},
                                {'empty': self.root}, ('empty',), ('pinyin',), None)
        self.assertEqual(status, 3)

    def test_memory_and_timing_control_withhold_unstable_ratio(self):
        for kind, values in (('timing', {'base': [1]*4, 'pr': [1]*4, 'control': [2]*4}),
                             ('memory', {'base': [0]*4, 'pr': [0]*4, 'control': [65]*4})):
            with mock.patch.object(st, 'emit') as emit:
                st.report_values(kind, values, metric='rss')
            self.assertEqual(emit.call_args.kwargs['verdict'], 'INCONCLUSIVE')
            self.assertIsNone(emit.call_args.kwargs['pr_base_ratio'])
        with mock.patch.object(st, 'emit') as emit:
            st.report_values('memory', {'base': [None], 'pr': [None], 'control': [None]}, metric='pss')
        self.assertEqual(emit.call_args.kwargs['verdict'], 'unavailable')

    def memory_line(self, platform, metric, level, change):
        """The line printed for one memory metric: base and PR at `level` KiB, the control `change` KiB above."""
        values = {'base': [level] * 4, 'pr': [level] * 4, 'control': [level + change] * 4}
        out = io.StringIO()
        with mock.patch.object(st.sys, 'platform', platform), contextlib.redirect_stdout(out):
            st.report_values('memory', values, facade='pinyin', profile='empty', point='end', metric=metric, unit='KiB')
        return out.getvalue()

    def test_macos_virtual_size_is_not_informative_and_never_stable(self):
        # macOS virtual size includes the shared region (about 488 GB), so the §7.5 residency band, 1% of
        # the median (about 4.7 GiB), passes any change. Whatever the control does, the metric has no verdict.
        level = 500_000_000
        for change in (0, 65_536, 4_000_000, level):
            with self.subTest(change=change):
                line = self.memory_line('darwin', 'virtual_size', level, change)
                record = json.loads(line)
                self.assertEqual((record['verdict'], record['summaries'], record['pr_base_ratio']), ('unavailable', None, None))
                self.assertIn('not informative', record['reason'])
                self.assertNotIn('control', record)
                self.assertNotIn('STABLE', line)
                self.assertEqual(len(line.splitlines()), 1)

    def test_macos_resident_and_footprint_verdicts_use_darwin_names(self):
        # The Darwin reader reports these metrics; the PR/base ratio and control
        # rules are the same residency rules Linux uses for its own names.
        for metric in ('resident_kib', 'physical_footprint_kib',
                       'physical_footprint_lifetime_max_kib'):
            for change, verdict, ratio in ((50, 'STABLE', {'ci95': [1.0, 1.0], 'median': 1.0}),
                                           (200, 'INCONCLUSIVE', None)):
                with self.subTest(metric=metric, change=change):
                    line = self.memory_line('darwin', metric, 10_000, change)
                    record = json.loads(line)
                    self.assertEqual((record['verdict'], record['pr_base_ratio']), (verdict, ratio))
                    self.assertEqual(record['control']['stable'], verdict == 'STABLE')
                    self.assertEqual(line, self.memory_line('linux', metric, 10_000, change))

    def test_darwin_counts_are_diagnostic_without_a_ratio(self):
        for metric in ('faults', 'pageins', 'disk_bytes_read', 'disk_bytes_written'):
            with self.subTest(metric=metric), mock.patch.object(st, 'emit') as emit:
                st.report_values('memory', {'base': [1] * 4, 'pr': [1] * 4, 'control': [2] * 4},
                                 metric=metric)
            self.assertEqual(emit.call_args.kwargs['verdict'], 'diagnostic')
            self.assertIsNone(emit.call_args.kwargs['pr_base_ratio'])

    def test_linux_virtual_size_output_is_byte_identical_to_before(self):
        # Linux reads VmSize, so virtual size keeps its residency verdict. The digests are SHA-256 of the lines
        # the controller printed for these samples before macOS virtual size lost its verdict.
        before = {65_536: ('STABLE', 'de4d5a8f925771488af588a17ce751a78cc772e6cc2ef6dcb6dc180120ff26d7'),
                  6_000_000: ('INCONCLUSIVE', 'ecacd1ac1b13a906d1a4086f75e6e1b44a5645e748a56109cf55c02c1e751106')}
        for change, (verdict, digest) in before.items():
            with self.subTest(change=change):
                line = self.memory_line('linux', 'virtual_size', 500_000_000, change)
                self.assertEqual(json.loads(line)['verdict'], verdict)
                self.assertEqual(worker.sha256(line.encode()), digest, line)

    def test_tmpfs_and_multiple_apfs_volumes_are_refused(self):
        with mock.patch.object(native.sys, 'platform', 'linux'), \
                mock.patch.object(native, 'filesystem', return_value=('tmpfs', '0:1')):
            with self.assertRaisesRegex(ValueError, 'requires ext4'):
                native.check_filesystems([self.root])
        with mock.patch.object(native.sys, 'platform', 'darwin'), \
                mock.patch.object(native, 'filesystem', side_effect=[('apfs', 'A'), ('apfs', 'B')]):
            with self.assertRaisesRegex(ValueError, 'one APFS volume'):
                native.check_filesystems([self.root / 'base', self.root / 'pr'])

    def test_apfs_volume_lookup_uses_the_path_filesystem_device(self):
        import plistlib
        with mock.patch.object(native.sys, 'platform', 'darwin'), \
                mock.patch.object(native.subprocess, 'check_output', return_value='Filesystem 512-blocks Used Available Capacity Mounted on\n/dev/disk3s1 100 20 80 20% /System/Volumes/Data\n'), \
                mock.patch.object(native.subprocess, 'run', return_value=SimpleNamespace(stdout=plistlib.dumps(
                    dict(FilesystemType='apfs', VolumeUUID='data-volume')))) as run:
            self.assertEqual(native.filesystem(self.root), ('apfs', 'data-volume'))
        self.assertEqual(run.call_args.args[0], ['diskutil', 'info', '-plist', '/dev/disk3s1'])

    def test_linux_parent_memory_fields(self):
        proc = self.root / '123'
        proc.mkdir()
        (proc / 'status').write_text('VmRSS: 8 kB\nVmHWM: 10 kB\nVmSize: 100 kB\n')
        (proc / 'smaps_rollup').write_text('Rss: 8 kB\nAnonymous: 3 kB\nPss: 6 kB\nPrivate_Dirty: 2 kB\n')
        (proc / 'stat').write_text('123 (a ) long name) S ' + ' '.join(str(i) for i in range(4, 40)))
        # The /proc fixture is Linux's reader; select it explicitly so a macOS host does not run ps on pid 123.
        with mock.patch.object(native.sys, 'platform', 'linux'):
            memory = native.read_memory(123, proc_root=self.root)
        self.assertEqual((memory['rss'], memory['file_backed'], memory['minor_faults'], memory['major_faults']), (8, 5, 10, 12))
        self.assertEqual((memory['peak_rss'], memory['virtual_size'], memory['anonymous'], memory['private_dirty'], memory['pss']),
                         (10, 100, 3, 2, 6))
        self.assertIsNone(memory['rss_delta'])

    def test_darwin_binding_fields_map_to_darwin_names_in_kib(self):
        usage = SimpleNamespace(
            ri_resident_size=8 * 1024,
            ri_phys_footprint=10 * 1024,
            ri_interval_max_phys_footprint=11 * 1024,
            ri_lifetime_max_phys_footprint=12 * 1024,
            ri_diskio_bytesread=13,
            ri_diskio_byteswritten=14,
            ri_instructions=17,
            ri_cycles=18,
        )
        info = SimpleNamespace(pti_faults=15, pti_pageins=16)
        with mock.patch.object(native.sys, 'platform', 'darwin'), \
                mock.patch.object(native.darwin, 'rusage', return_value=usage), \
                mock.patch.object(native.darwin, 'taskinfo', return_value=info), \
                mock.patch.object(native.darwin, 'resident_peak', return_value=9 * 1024), \
                mock.patch.object(native.darwin, 'counter_availability', return_value=(True, True)), \
                mock.patch.object(native.subprocess, 'check_output') as ps:
            memory = native.read_memory(123)
            self_observed = native.read_memory(123, resident_peak_bytes=20 * 1024)
        ps.assert_not_called()
        self.assertEqual(self_observed['resident_peak_kib'], 20)
        self.assertEqual((memory['resident_kib'], memory['resident_peak_kib']), (8, 9))
        self.assertEqual((memory['physical_footprint_kib'],
                          memory['physical_footprint_interval_max_kib'],
                          memory['physical_footprint_lifetime_max_kib']), (10, 11, 12))
        self.assertEqual((memory['faults'], memory['pageins']), (15, 16))
        self.assertEqual((memory['disk_bytes_read'], memory['disk_bytes_written']), (13, 14))
        self.assertEqual((memory['instructions'], memory['cycles']), (17, 18))
        self.assertTrue(memory['instructions_available'])
        self.assertTrue(memory['cycles_available'])
        for field in native.LINUX_MEMORY_FIELDS:
            self.assertIsNone(memory[field], field)

    def test_darwin_unavailable_call_reports_none_not_zero(self):
        with mock.patch.object(native.sys, 'platform', 'darwin'), \
                mock.patch.object(native.darwin, 'rusage', side_effect=OSError(2, 'gone')), \
                mock.patch.object(native.darwin, 'taskinfo', side_effect=OSError(2, 'gone')), \
                mock.patch.object(native.darwin, 'resident_peak', return_value=None), \
                mock.patch.object(native.darwin, 'counter_availability', return_value=(True, True)):
            memory = native.read_memory(123)
        self.assertTrue(all(memory[field] is None for field in native.DARWIN_MEMORY_FIELDS))
        self.assertTrue(all(memory[field] is None for field in native.LINUX_MEMORY_FIELDS))
        self.assertIsNone(memory['instructions'])
        self.assertIsNone(memory['cycles'])
        self.assertFalse(memory['instructions_available'])
        self.assertFalse(memory['cycles_available'])

    def test_darwin_unavailable_counters_are_none_not_zero(self):
        usage = SimpleNamespace(
            ri_resident_size=8 * 1024, ri_phys_footprint=10 * 1024,
            ri_interval_max_phys_footprint=11 * 1024,
            ri_lifetime_max_phys_footprint=12 * 1024,
            ri_diskio_bytesread=13, ri_diskio_byteswritten=14,
            ri_instructions=0, ri_cycles=0,
        )
        info = SimpleNamespace(pti_faults=15, pti_pageins=16)
        with mock.patch.object(native.sys, 'platform', 'darwin'), \
                mock.patch.object(native.darwin, 'rusage', return_value=usage), \
                mock.patch.object(native.darwin, 'taskinfo', return_value=info), \
                mock.patch.object(native.darwin, 'resident_peak', return_value=None), \
                mock.patch.object(native.darwin, 'counter_availability', return_value=(False, False)):
            memory = native.read_memory(123)
        self.assertIsNone(memory['instructions'])
        self.assertIsNone(memory['cycles'])
        self.assertFalse(memory['instructions_available'])
        self.assertFalse(memory['cycles_available'])

    def test_darwin_counter_probe_requires_each_counter_to_advance(self):
        def usage(instructions, cycles):
            return SimpleNamespace(ri_instructions=instructions, ri_cycles=cycles)

        cases = (
            ((10, 20), (30, 40), (True, True)),
            ((0, 0), (0, 0), (False, False)),
            ((10, 0), (30, 0), (True, False)),
            ((0, 20), (0, 40), (False, True)),
        )
        for before, after, expected in cases:
            with self.subTest(before=before, after=after):
                with mock.patch.object(native.darwin, 'rusage',
                                       side_effect=[usage(*before), usage(*after)]):
                    self.assertEqual(native.darwin._probe_counters(sleep=lambda _: None), expected)

    @unittest.skipUnless(sys.platform == 'darwin', 'Darwin libproc binding')
    def test_darwin_binding_reads_this_process(self):
        observed = native.darwin.observation(os.getpid())
        self.assertGreater(observed['resident_bytes'], 0)
        self.assertGreater(observed['physical_footprint_bytes'], 0)
        self.assertGreater(observed['physical_footprint_lifetime_max_bytes'], 0)
        self.assertGreaterEqual(observed['faults'], 0)
        self.assertGreaterEqual(observed['pageins'], 0)
        self.assertGreaterEqual(observed['disk_bytes_read'], 0)
        self.assertGreaterEqual(observed['disk_bytes_written'], 0)
        self.assertTrue(observed['resident_peak_bytes'] is None
                        or isinstance(observed['resident_peak_bytes'], int))
        self.assertIsInstance(observed['instructions_available'], bool)
        self.assertIsInstance(observed['cycles_available'], bool)
        self.assertTrue(observed['instructions'] is None
                        or isinstance(observed['instructions'], int))
        self.assertTrue(observed['cycles'] is None
                        or isinstance(observed['cycles'], int))

    def test_raw_files_are_not_normalized_and_database_rows_are_not_read(self):
        (self.root / 'user.bin').write_bytes(b'1234timestamp')
        (self.root / 'user.db').write_bytes(b'db container')
        files = worker.userfiles(self.root)
        self.assertEqual(files['user.bin'], {'raw_hex': b'1234timestamp'.hex()})
        self.assertEqual(files['user.db'], {'database': True})


class WorkerIntegration(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temporary.name)
        helper = st.module(ROOT / 'tools/workers/startup.test.py', 'startup_test_helper')
        cls.stub_exports = staticmethod(helper.stub_exports)
        cls.executable, cls.identity = worker.build(cls.root, dict(os.environ), ())
        cls.stub = helper.build_stub(cls.root)
        cls.data = cls.root / 'data'
        cls.data.mkdir()
        cls.profile = cls.root / 'profile'
        cls.profile.mkdir()
        cls.campaign = worker.Worker(cls.executable, cls.identity, cls.data, cls.root, dict(os.environ), (), None, 'now')

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def setUp(self):
        self.enterContext(mock.patch.object(worker, 'exports', self.stub_exports))

    def test_parent_reads_live_stopped_child_at_every_handshake(self):
        pids, peaks = [], []
        def read(pid, **kwargs):
            os.kill(pid, 0)
            pids.append(pid)
            peaks.append(kwargs.get('resident_peak_bytes'))
            return dict(rss=10)
        with mock.patch.object(native, 'read_memory', side_effect=read):
            row = self.campaign.run(self.stub, 'pinyin', self.profile, memory=True)
        self.assertEqual(tuple(row['checkpoints']), worker.POINTS)
        self.assertEqual(len(pids), 7)
        self.assertEqual(len(set(pids)), 1)
        self.assertNotEqual(pids[0], os.getpid())
        if sys.platform == 'darwin':
            self.assertTrue(all(isinstance(peak, int) and peak > 0 for peak in peaks))
        else:
            self.assertEqual(peaks, [None] * len(worker.POINTS))
        verify = self.campaign.run(self.stub, 'pinyin', self.profile, mode='verify')
        self.assertEqual(verify['digest'], row['digest'])
        self.assertFalse((self.root / 'sample').exists())

    def test_parent_reads_the_live_stopped_child_memory_at_each_handshake(self):
        # No mock: /proc on Linux and the shared Darwin libproc binding on macOS
        # run against the worker while it blocks on each handshake.
        row = self.campaign.run(self.stub, 'pinyin', self.profile, memory=True)
        self.assertEqual(tuple(row['checkpoints']), worker.POINTS)
        for point, memory in row['checkpoints'].items():
            with self.subTest(point=point):
                expected = set(native.MEMORY_FIELDS)
                if sys.platform == 'darwin':
                    expected |= set(native.DARWIN_COUNTER_FIELDS)
                    expected |= set(native.DARWIN_AVAILABILITY_FIELDS)
                self.assertEqual(set(memory), expected)
                if sys.platform == 'darwin':
                    self.assertGreater(memory['resident_kib'], 0)
                    self.assertGreater(memory['resident_peak_kib'], 0)
                    self.assertGreater(memory['physical_footprint_kib'], 0)
                    self.assertGreaterEqual(memory['faults'], 0)
                    self.assertGreaterEqual(memory['pageins'], 0)
                    self.assertIsInstance(memory['instructions_available'], bool)
                    self.assertIsInstance(memory['cycles_available'], bool)
                    if memory['instructions_available']:
                        self.assertIsInstance(memory['instructions'], int)
                    if memory['cycles_available']:
                        self.assertIsInstance(memory['cycles'], int)
                    for field in native.LINUX_MEMORY_FIELDS:
                        self.assertIsNone(memory[field], field)
                else:
                    self.assertGreater(memory['rss'], 0)
                    self.assertGreater(memory['virtual_size'], 0)
                    self.assertEqual(sorted(k for k in native.MEMORY_FIELDS if memory[k] is None),
                                     sorted(['rss_delta', *native.DARWIN_MEMORY_FIELDS]))

    def test_sequential_copies_use_identical_user_path_without_stderr_normalization(self):
        paths = []
        execute = worker.execute
        def track(command, **kwargs):
            paths.append(command[-3])
            return execute(command, **kwargs)
        with mock.patch.object(worker, 'execute', side_effect=track):
            for _ in range(2):
                self.campaign.run(self.stub, 'pinyin', self.profile)
        self.assertEqual(paths[0], paths[1])
        self.assertFalse((self.root / 'sample').exists())

    def test_worker_identity_tamper_refuses_before_execution(self):
        for change in (dict(sha256='wrong'), dict(command=[]), dict(source_sha256='wrong'), dict(bytes=1)):
            with self.assertRaises(ValueError):
                worker.check_identity(self.executable, dict(self.identity, **change))

    def test_steady_scope_uses_existing_worker_and_preserves_unit_digest(self):
        execute = worker.execute
        def run(command, **kwargs):
            if command[0] == 'valgrind':
                self.assertIn('--toggle-collect=startup_steady_unit', command)
                self.assertEqual(kwargs['env']['OXPINYIN_STARTUP_STEADY'], '1')
                command = command[command.index(str(self.executable)):]
            else:
                # Not assertNotIn(…, env): its failure message would print the whole environment.
                self.assertIsNone(kwargs['env'].get('OXPINYIN_STARTUP_STEADY'))
            return execute(command, **kwargs)
        with mock.patch.dict(self.campaign.env, OXPINYIN_STARTUP_STEADY='inherited'), \
                mock.patch.object(worker, 'execute', side_effect=run):
            normal = self.campaign.run(self.stub, 'pinyin', self.profile)
            steady = self.campaign.run(self.stub, 'pinyin', self.profile, scope='steady')
        self.assertEqual(normal['digest'], steady['digest'])
        if sys.platform == 'linux':
            loaded = [d for d in steady['documents'] if d.get('startup') == 'loaded_object']
            self.assertTrue(loaded)
            self.assertTrue(all(d['pid'] == steady['pid'] for d in loaded))
            self.assertTrue(any(d['object'] == str(self.stub) for d in loaded))
            self.assertTrue(all(d['segments'] for d in loaded))

    def controller_with_anonymous_profile(self, *, ambiguous=False):
        # Execute the real fixture worker and forward its actual same-process
        # loader output. Inject only a profiler-format fixture, not a measurement.
        execute = worker.execute
        baseline = self.campaign.run(self.stub, 'pinyin', self.profile)
        def profiled(command, **kwargs):
            native_command = command[command.index(str(self.executable)):]
            row = execute(native_command, **kwargs)
            documents = [json.loads(line) for line in row['stdout'].splitlines()]
            loaded = [d for d in documents if d.get('startup') == 'loaded_object']
            header = (f'creator: callgrind-3.27.1\npid: {row["pid"]}\n'
                      f'cmd:  {self.executable} measure\npositions: instr line\nevents: Ir\nsummary: 100000\n')
            if not loaded:  # Exact init intentionally needs no attribution.
                body = 'ob=???\nfn=0x4aad720\n0x4aad720 0 100000\n'
            else:
                obj = next(d for d in loaded if d['object'] == str(self.stub))
                segment = next(s for s in obj['segments'] if s['flags'] & 1)
                address = segment['start'] + 16
                # Preserve the reported compression/edge shape: callee-only
                # 396 Ir is inclusive and excluded, then 35 exclusive sites
                # with 1226 Ir in the anonymous object.
                body = (f'ob=(1) {self.stub}\nfn=stable\n0x20 0 98774\n'
                        f'cob=(3) ???\ncfn=(2238) {hex(address + 512)}\n'
                        f'calls=18 {hex(address + 512)} 0\n0x20 0 396\n'
                        f'ob=(3)\nfl=(161) ???\nfn=(2130) {hex(address)}\n')
                costs = [28] + [35] * 33 + [43]
                body += ''.join(f'{hex(address + i)} 0 {cost}\n' for i, cost in enumerate(costs))
                if ambiguous:
                    # Same PID, overlapping claimed mapping: must refuse and
                    # retain both loader records alongside the raw profile.
                    row['stdout'] += (json.dumps(obj) + '\n').encode()
            row['stderr'] = (header + body + 'totals: 100000\n').encode()
            return row
        records = []
        libraries = {arm: {'pinyin': self.stub} for arm in st.ARMS}
        with mock.patch.object(worker, 'execute', side_effect=profiled), \
                mock.patch.object(st, 'emit', side_effect=lambda kind, **values: records.append(values)):
            try:
                st.instrument(self.campaign, libraries, 'pinyin', 'empty', self.profile,
                              {arm: dict(digest=baseline['digest']) for arm in st.ARMS})
            except st.NotResult:
                if not ambiguous:
                    raise
        return records

    @unittest.skipUnless(sys.platform == 'linux', 'Linux loader records')
    def test_anonymous_exclusive_sites_reach_controller_with_live_loader_records(self):
        records = self.controller_with_anonymous_profile()
        self.assertEqual([r['verdict'] for r in records], ['VALID', 'RANGE-QUALIFIED', 'RANGE-QUALIFIED'])
        for row in records[1:]:
            self.assertEqual(row['constant_unresolved_count'], 35)
            self.assertEqual(row['constant_unresolved_Ir'], 1226)
            for samples in row['profiles'].values():
                for sample in samples:
                    self.assertTrue(sample['parsed'])
                    self.assertTrue(sample['loader_data'])
                    self.assertTrue(all(d['pid'] == sample['pid'] for d in sample['loader_data']))

    @unittest.skipUnless(sys.platform == 'linux', 'Linux loader records')
    def test_invalid_anonymous_attribution_retains_live_loader_and_raw_sample(self):
        records = self.controller_with_anonymous_profile(ambiguous=True)
        self.assertEqual(records[-1]['verdict'], 'INVALID')
        samples = [p for values in records[-1]['profiles'].values() for p in values]
        self.assertEqual(len(samples), 1)
        sample = samples[0]
        self.assertFalse(sample['parsed'])
        self.assertTrue(sample['loader_data'])
        self.assertIn('cob=(3) ???', bytes.fromhex(sample['raw_profile_hex']).decode())
        self.assertIn(b'loaded_object', bytes.fromhex(sample['raw_stdout_hex']))
        self.assertIn('ambiguous loader mapping', records[-1]['reason'])

    def test_timeout_reaps_worker(self):
        # A real worker verifies process-group termination and reaping on timeout.
        with self.assertRaisesRegex(ValueError, 'timed out'):
            worker.execute([sys.executable, '-c', 'import time; time.sleep(10)'], env=dict(os.environ), timeout=.05)


if __name__ == '__main__':
    unittest.main()
