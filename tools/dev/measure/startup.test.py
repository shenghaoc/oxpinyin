#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
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
from tools.dev import cli, slot
from tools.dev.measure import startup as st, worker, native, stats


class ControllerTests(unittest.TestCase):
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
        admission.assert_not_called()
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

    def test_macos_cold_and_windows_are_refused_before_filesystem_or_slot(self):
        for system, extra in (('darwin', ['--cache', 'cold']), ('win32', [])):
            with mock.patch.object(st.sys, 'platform', system), mock.patch.object(slot, 'HeavyAdmission') as admission, \
                    contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(st.main([*self.argv, *extra]), 1)
            admission.assert_not_called()

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
        # Exercise every metadata/memory command, including both otool options.
        calls = [
            ('darwin', native.subprocess, 'check_output', lambda: native.filesystem(self.root), ['df']),
            ('darwin', native.subprocess, 'run', lambda: native.filesystem(self.root), ['diskutil']),
            ('darwin', native.subprocess, 'check_output', lambda: native.read_memory(123), ['ps']),
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
        admission.assert_not_called()
        self.assertIn('one APFS volume', error.getvalue())

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
        def run(path, facade, profile, *, memory=False):
            seen.append((path.parent.name, memory))
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

    def test_macos_parent_memory_is_ps_rss_and_virtual_size_only(self):
        # §7.7: on macOS the parent reads RSS and virtual size with ps; every other field stays unavailable.
        with mock.patch.object(native.sys, 'platform', 'darwin'), \
                mock.patch.object(native.subprocess, 'check_output', return_value=' 8 100\n') as ps:
            memory = native.read_memory(123)
        self.assertEqual(ps.call_args.args[0], ['ps', '-o', 'rss=', '-o', 'vsz=', '-p', '123'])
        self.assertEqual((memory['rss'], memory['virtual_size']), (8, 100))
        self.assertTrue(all(memory[k] is None for k in native.MEMORY_FIELDS if k not in ('rss', 'virtual_size')))
        with mock.patch.object(native.sys, 'platform', 'darwin'), \
                mock.patch.object(native.subprocess, 'check_output', return_value='\n'), self.assertRaises(ValueError):
            native.read_memory(123)

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
        pids = []
        def read(pid):
            os.kill(pid, 0)
            pids.append(pid)
            return dict(rss=10)
        with mock.patch.object(native, 'read_memory', side_effect=read):
            row = self.campaign.run(self.stub, 'pinyin', self.profile, memory=True)
        self.assertEqual(tuple(row['checkpoints']), worker.POINTS)
        self.assertEqual(len(pids), 7)
        self.assertEqual(len(set(pids)), 1)
        self.assertNotEqual(pids[0], os.getpid())
        verify = self.campaign.run(self.stub, 'pinyin', self.profile, mode='verify')
        self.assertEqual(verify['digest'], row['digest'])
        self.assertFalse((self.root / 'sample').exists())

    def test_parent_reads_the_live_stopped_child_memory_at_each_handshake(self):
        # No mock: the host reader (/proc on Linux, ps on macOS, §7.7) runs against the worker
        # while it blocks on each handshake, and must return that process's residency.
        row = self.campaign.run(self.stub, 'pinyin', self.profile, memory=True)
        self.assertEqual(tuple(row['checkpoints']), worker.POINTS)
        for point, memory in row['checkpoints'].items():
            with self.subTest(point=point):
                self.assertEqual(set(memory), set(native.MEMORY_FIELDS))
                self.assertGreater(memory['rss'], 0)
                self.assertGreater(memory['virtual_size'], 0)
                unavailable = sorted(k for k in native.MEMORY_FIELDS if memory[k] is None)
                if sys.platform == 'darwin':
                    self.assertEqual(unavailable, sorted(k for k in native.MEMORY_FIELDS if k not in ('rss', 'virtual_size')))
                else:
                    self.assertEqual(unavailable, ['rss_delta'])

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
                self.assertNotIn('OXPINYIN_STARTUP_STEADY', kwargs['env'])
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
        with self.assertRaisesRegex(ValueError, 'timed out'):
            worker.execute([sys.executable, '-c', 'import time; time.sleep(10)'], env=dict(os.environ), timeout=.05)


if __name__ == '__main__':
    unittest.main()
