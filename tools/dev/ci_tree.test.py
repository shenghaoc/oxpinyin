#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tree identity, provenance, incomplete evidence and aggregate regressions."""
from copy import deepcopy
from datetime import datetime, timedelta, timezone
import contextlib
import io
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev import ci_tree

TREE = 'd035ee52' + '0' * 32
HEAD = 'b2ae9126' + '0' * 32
REBASED = 'a04c91af' + '0' * 32
PROFILE = ci_tree.WORKFLOWS['ci']
NOW = datetime(2026, 10, 26, 12, tzinfo=timezone.utc)


def every(flag, profile=PROFILE):
    """The same path-gate flag for every lane of profile."""
    return dict.fromkeys(profile.lanes, flag)


def outputs_with(flag, profile, **rest):
    """`changes` outputs that set each lane's flag to flag."""
    return {**dict.fromkeys(profile.flags, flag), **rest}


def successful_job(name):
    return dict(name=name, status='completed', conclusion='success',
                started_at=(NOW - timedelta(hours=1)).isoformat(),
                completed_at=(NOW - timedelta(minutes=30)).isoformat())


def run_fixture():
    return dict(id=100, event='pull_request', path=PROFILE.path, head_sha=HEAD,
                repository={'id': 1}, head_repository={'id': 1}, status='completed',
                conclusion='success', run_attempt=1, pull_requests=[{'number': 2}])


def job_fixture():
    return [successful_job(name)
            for name in (*PROFILE.lanes, PROFILE.aggregate, 'tested tree ' + TREE)]


class API:
    def __init__(self):
        self.run = run_fixture()
        self.jobs = job_fixture()
        self.tree = TREE
        self.runs = None
        self.prs = None
        self.get_paths = []

    def get(self, path):
        self.get_paths.append(path)
        if path.startswith('/git/commits/'):
            return {'tree': {'sha': self.tree}}
        raise AssertionError(path)

    def pages(self, path, key=None):
        if path.startswith('/commits/'):
            return self.prs if self.prs is not None else [pr_fixture()]
        if '/runs?' in path:
            return self.runs if self.runs is not None else [self.run]
        if '/attempts/' in path:
            return self.jobs
        raise AssertionError(path)


def pr_fixture():
    return dict(number=2, merged_at='2026-10-26', base={'ref': 'main', 'repo': {'id': 1}},
                head={'sha': HEAD, 'ref': 'wip/1299', 'repo': {'id': 1}})


class EvidenceTests(unittest.TestCase):
    def evidence(self, api, event_name='push'):
        event = {'ref': 'refs/heads/main', 'after': REBASED, 'pull_request': pr_fixture()}
        return ci_tree.find_evidence(api, PROFILE, event, event_name, TREE, 1, 200, now=NOW)

    def test_rebase_merge_different_commit_same_tree_reuses(self):
        self.assertNotEqual(HEAD, REBASED)
        self.assertEqual(self.evidence(API()), 100)

    def test_same_tree_on_own_pr_reuses(self):
        self.assertEqual(self.evidence(API(), 'pull_request'), 100)


    def test_changed_tree_runs_full_ci(self):
        api = API()
        api.tree = 'f' * 40
        self.assertIsNone(self.evidence(api))

    def test_missing_failed_skipped_pending_or_duplicate_jobs_rejected(self):
        for index in range(len(job_fixture())):
            for status in ('failure', 'cancelled', 'skipped', None):
                api = API()
                api.jobs[index]['conclusion'] = status
                with self.subTest(index=index, status=status):
                    self.assertIsNone(self.evidence(api))
            api = API()
            del api.jobs[index]
            self.assertIsNone(self.evidence(api))
            api = API()
            api.jobs.append(deepcopy(api.jobs[index]))
            self.assertIsNone(self.evidence(api))

    def test_untrusted_run_rejected(self):
        changes = {'event': 'push', 'path': '.github/workflows/other.yml',
                   'head_sha': REBASED, 'repository': {'id': 3},
                   'head_repository': {'id': 3}, 'status': 'in_progress',
                   'conclusion': 'failure', 'run_attempt': None, 'pull_requests': []}
        for key, value in changes.items():
            run = run_fixture()
            run[key] = value
            with self.subTest(key=key):
                self.assertFalse(ci_tree.trusted_run(run, PROFILE, 1, 2, HEAD, 200))

    def test_newer_incomplete_or_failed_same_tree_cannot_resurrect_green(self):
        for status, conclusion in [('in_progress', None), ('completed', 'failure'),
                                    ('completed', 'cancelled')]:
            api = API()
            newer = run_fixture()
            newer.update(id=101, status=status, conclusion=conclusion)
            api.runs = [api.run, newer]
            self.assertIsNone(self.evidence(api))

    def test_current_run_is_not_its_own_evidence(self):
        api = API()
        api.run['id'] = 200
        self.assertIsNone(self.evidence(api))

    def test_checkout_marker_is_actual_tested_tree(self):
        api = API()
        api.jobs[-1]['name'] = 'tested tree ' + 'f' * 40
        self.assertIsNone(self.evidence(api))

    def test_latest_attempt_jobs_only(self):
        api = API()
        api.run['run_attempt'] = 2
        api.jobs[-1]['conclusion'] = 'skipped'
        self.assertIsNone(self.evidence(api))

    def test_reused_run_resolves_to_original_full_evidence(self):
        api = API()
        latest = run_fixture()
        latest['id'] = 101
        jobs = [dict(name=name, status='completed', conclusion='skipped') for name in PROFILE.lanes]
        jobs += [dict(name=PROFILE.aggregate, status='completed', conclusion='success'),
                 dict(name='reused tree ' + TREE + ' from run 100', status='completed', conclusion='success')]
        original_get = api.get
        api.get = lambda path: run_fixture() if path == '/actions/runs/100' else original_get(path)
        self.assertEqual(ci_tree.full_source(api, latest, jobs, PROFILE, TREE, 1, 2, 200, now=NOW), 100)
        api.jobs[0]['conclusion'] = 'failure'
        self.assertIsNone(ci_tree.full_source(api, latest, jobs, PROFILE, TREE, 1, 2, 200, now=NOW))

    def test_called_backend_evidence_uses_caller_run_and_only_its_own_jobs(self):
        profile = ci_tree.WORKFLOWS['store']
        api = API()
        backend_jobs = [successful_job(profile.job_prefix + name)
                        for name in (*profile.lanes, profile.aggregate, 'tested tree ' + TREE)]
        api.jobs += backend_jobs
        event = {'pull_request': pr_fixture()}
        with mock.patch.object(api, 'pages', wraps=api.pages) as pages:
            self.assertEqual(ci_tree.find_evidence(api, profile, event, 'pull_request',
                                                   TREE, 1, 200, now=NOW), 100)
            self.assertTrue(any(call.args[0].startswith('/actions/workflows/ci.yml/runs?')
                                for call in pages.call_args_list))
        for job in backend_jobs:
            job['conclusion'] = 'failure'
            self.assertFalse(ci_tree.complete_jobs(api.jobs, profile, TREE, NOW))
            job['conclusion'] = 'success'
        api.run['path'] = profile.path
        self.assertIsNone(ci_tree.find_evidence(api, profile, event, 'pull_request',
                                               TREE, 1, 200, now=NOW))

    def test_age_boundary_is_measured_from_each_original_job_start(self):
        for index in range(len(job_fixture())):
            api = API()
            job = api.jobs[index]
            job['started_at'] = (NOW - ci_tree.EVIDENCE_MAX_AGE).isoformat()
            self.assertEqual(self.evidence(api), 100)
            job['started_at'] = (NOW - ci_tree.EVIDENCE_MAX_AGE - timedelta(microseconds=1)).isoformat()
            self.assertIsNone(self.evidence(api))

    def test_missing_malformed_naive_future_or_reversed_timestamps_rejected(self):
        for key in ('started_at', 'completed_at'):
            for value in (None, '', 'invalid', '2026-10-26T11:00:00',
                          (NOW + timedelta(seconds=1)).isoformat()):
                api = API()
                api.jobs[0][key] = value
                with self.subTest(key=key, value=value):
                    self.assertIsNone(self.evidence(api))
            api = API()
            del api.jobs[0][key]
            self.assertIsNone(self.evidence(api))
        api = API()
        api.jobs[0]['completed_at'] = (NOW - timedelta(hours=2)).isoformat()
        self.assertIsNone(self.evidence(api))

    def test_fresh_duplicate_does_not_renew_expired_original_verification(self):
        api = API()
        latest = run_fixture()
        latest['id'] = 101
        jobs = [dict(name=name, status='completed', conclusion='skipped') for name in PROFILE.skipped_lanes]
        jobs += [successful_job(PROFILE.aggregate),
                 successful_job('reused tree ' + TREE + ' from run 100')]
        original_get = api.get
        api.get = lambda path: run_fixture() if path == '/actions/runs/100' else original_get(path)
        api.jobs[0]['started_at'] = (NOW - timedelta(days=2)).isoformat()
        self.assertIsNone(ci_tree.full_source(api, latest, jobs, PROFILE, TREE, 1, 2, 200, now=NOW))

    def test_null_or_missing_run_repository_is_no_evidence(self):
        for key in ('repository', 'head_repository'):
            for missing in (False, True):
                api = API()
                if missing:
                    del api.run[key]
                else:
                    api.run[key] = None
                with self.subTest(key=key, missing=missing):
                    self.assertIsNone(self.evidence(api))
        for value in (None, [], [None]):
            api = API()
            api.run['pull_requests'] = value
            self.assertIsNone(self.evidence(api))

    def test_null_or_missing_pr_repository_is_no_evidence(self):
        for side in ('base', 'head'):
            for missing in (False, True):
                api = API()
                pr = pr_fixture()
                if missing:
                    del pr[side]['repo']
                else:
                    pr[side]['repo'] = None
                api.prs = [pr]
                with self.subTest(side=side, missing=missing):
                    self.assertIsNone(self.evidence(api))
            api = API()
            pr = pr_fixture()
            pr[side] = None
            api.prs = [pr]
            self.assertIsNone(self.evidence(api))

    def test_deleted_fork_objects_emit_normal_ci_outputs(self):
        for deleted in ('run', 'pr'):
            api = API()
            if deleted == 'run':
                api.run['head_repository'] = None
            else:
                pr = pr_fixture()
                pr['head']['repo'] = None
                api.prs = [pr]
            with tempfile.TemporaryDirectory() as directory:
                output = Path(directory, 'output')
                summary = Path(directory, 'summary')
                event = Path(directory, 'event.json')
                event.write_text(json.dumps({'ref': 'refs/heads/main', 'after': REBASED}))
                env = {'GITHUB_REPOSITORY': 'owner/repo', 'GH_TOKEN': 'unit-test-placeholder',
                       'GITHUB_EVENT_NAME': 'push', 'GITHUB_EVENT_PATH': str(event),
                       'GITHUB_REPOSITORY_ID': '1', 'GITHUB_RUN_ID': '200',
                       'GITHUB_OUTPUT': str(output), 'GITHUB_STEP_SUMMARY': str(summary)}
                with mock.patch.dict(ci_tree.os.environ, env, clear=True), \
                        mock.patch.object(ci_tree, 'GitHub', return_value=api), \
                        mock.patch.object(ci_tree, 'git_tree', return_value=TREE), \
                        mock.patch.object(sys, 'argv', ['ci_tree.py', 'evidence', 'ci']), \
                        contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(ci_tree.main(), 0)
                self.assertIn('reuse=false\n', output.read_text())
                self.assertIn('normal path gates apply', summary.read_text())

    def test_same_named_fork_branch_is_filtered_before_tree_lookup(self):
        for foreign in ({'head_repository': {'id': 3}},
                        {'repository': {'id': 3}},
                        {'pull_requests': [{'number': 3}]},
                        {'head_repository': None}):
            api = API()
            other = {**run_fixture(), **foreign, 'id': 101, 'head_sha': 'f' * 40,
                     'head_branch': 'wip/1299'}
            api.runs = [other]
            self.assertIsNone(self.evidence(api))
            api.runs = [other, api.run]
            self.assertEqual(self.evidence(api), 100)
            self.assertNotIn('/git/commits/' + 'f' * 40, api.get_paths)
        api = API()
        api.runs = [{**run_fixture(), 'id': index, 'head_repository': {'id': 3}}
                    for index in range(101, 140)] + [api.run]
        self.assertEqual(self.evidence(api), 100)


def source_jobs(ran):
    """A source run's jobs: the lanes in ran succeeded, every other lane was skipped."""
    lanes = [successful_job(name) if name in ran else dict(name=name, status='completed', conclusion='skipped')
             for name in PROFILE.lanes]
    return lanes + [successful_job(PROFILE.aggregate), successful_job('tested tree ' + TREE)]


class SelectedLaneEvidenceTests(unittest.TestCase):
    """Reuse needs source evidence for each lane the current run selects, and for no other lane."""

    def evidence(self, ran, selected, change=None, event_name='push'):
        api = API()
        api.jobs = source_jobs(ran)
        if change:
            change(api.jobs)
        event = {'ref': 'refs/heads/main', 'after': REBASED, 'pull_request': pr_fixture()}
        return ci_tree.find_evidence(api, PROFILE, event, event_name, TREE, 1, 200, now=NOW,
                                     lanes=tuple(selected))

    def test_the_truth_table_of_source_and_current_selections(self):
        rust, everything = ('lint', 'test'), PROFILE.lanes
        cases = (
            # (lanes the source ran, lanes the current run selects, reused)
            (rust, rust, True),
            (rust, ('lint',), True),
            (everything, rust, True),
            (everything, everything, True),
            (everything, (), True),
            ((), (), True),
            (rust, ('lint', 'test', 'deny'), False),
            (rust, everything, False),
            (rust, ('harness',), False),
            (('lint',), rust, False),
            ((), ('deny',), False),
        )
        for ran, selected, reused in cases:
            for event_name in ('push', 'pull_request'):
                with self.subTest(ran=ran, selected=selected, event_name=event_name):
                    self.assertEqual(self.evidence(ran, selected, event_name=event_name), 100 if reused else None)

    def test_a_selected_lane_that_did_not_succeed_freshly_once_never_authorizes_reuse(self):
        selected = ('lint', 'test')

        def job(jobs, name):
            return next(job for job in jobs if job['name'] == name)
        changes = {
            'failure': lambda jobs, name: job(jobs, name).update(conclusion='failure'),
            'cancelled': lambda jobs, name: job(jobs, name).update(conclusion='cancelled'),
            'skipped': lambda jobs, name: job(jobs, name).update(conclusion='skipped'),
            'no conclusion': lambda jobs, name: job(jobs, name).update(conclusion=None),
            'in progress': lambda jobs, name: job(jobs, name).update(status='in_progress'),
            'missing': lambda jobs, name: jobs.remove(job(jobs, name)),
            'duplicate': lambda jobs, name: jobs.append(deepcopy(job(jobs, name))),
            'stale': lambda jobs, name: job(jobs, name).update(
                started_at=(NOW - ci_tree.EVIDENCE_MAX_AGE - timedelta(seconds=1)).isoformat()),
            'no timestamp': lambda jobs, name: job(jobs, name).pop('started_at'),
        }
        for name in (*selected, PROFILE.aggregate, 'tested tree ' + TREE):
            for label, change in changes.items():
                with self.subTest(name=name, change=label):
                    self.assertIsNone(self.evidence(PROFILE.lanes, selected, lambda jobs: change(jobs, name)))

    def test_a_lane_the_current_run_does_not_select_is_not_read(self):
        for conclusion in ('skipped', 'failure', 'cancelled', None):
            with self.subTest(conclusion=conclusion):
                def change(jobs):
                    for job in jobs:
                        if job['name'] in ('deny', 'harness'):
                            job['conclusion'] = conclusion
                self.assertEqual(self.evidence(PROFILE.lanes, ('lint', 'test'), change), 100)
        self.assertEqual(self.evidence(PROFILE.lanes, ('lint', 'test'),
                                       lambda jobs: jobs.__delitem__(slice(2, 4))), 100)

    def test_a_marker_for_another_tree_or_a_changed_tree_is_no_evidence(self):
        def other_marker(jobs):
            jobs[-1]['name'] = 'tested tree ' + 'f' * 40
        self.assertIsNone(self.evidence(('lint',), ('lint',), other_marker))
        api = API()
        api.jobs = source_jobs(('lint',))
        api.tree = 'f' * 40
        event = {'ref': 'refs/heads/main', 'after': REBASED}
        self.assertIsNone(ci_tree.find_evidence(api, PROFILE, event, 'push', TREE, 1, 200, now=NOW, lanes=('lint',)))

    def test_no_selection_requires_every_lane(self):
        api = API()
        api.jobs = source_jobs(('lint', 'test'))
        event = {'ref': 'refs/heads/main', 'after': REBASED}
        self.assertIsNone(ci_tree.find_evidence(api, PROFILE, event, 'push', TREE, 1, 200, now=NOW))
        self.assertFalse(ci_tree.complete_jobs(api.jobs, PROFILE, TREE, NOW))
        self.assertTrue(ci_tree.complete_jobs(api.jobs, PROFILE, TREE, NOW, ('lint', 'test')))

    def test_a_lane_outside_the_workflow_is_no_evidence(self):
        self.assertFalse(ci_tree.complete_jobs(job_fixture(), PROFILE, TREE, NOW, ('lint', 'shipping-sizes')))

    def test_a_reused_run_is_revalidated_against_the_current_selection(self):
        api = API()
        api.jobs = source_jobs(('lint', 'test'))
        latest = {**run_fixture(), 'id': 101}
        jobs = [dict(name=name, status='completed', conclusion='skipped') for name in PROFILE.skipped_lanes]
        jobs += [successful_job(PROFILE.aggregate), successful_job('reused tree ' + TREE + ' from run 100')]
        original_get = api.get
        api.get = lambda path: run_fixture() if path == '/actions/runs/100' else original_get(path)

        def source(lanes):
            return ci_tree.full_source(api, latest, jobs, PROFILE, TREE, 1, 2, 200, now=NOW, lanes=lanes)
        self.assertEqual(source(('lint', 'test')), 100)
        self.assertEqual(source(()), 100)
        self.assertIsNone(source(('lint', 'test', 'deny')))
        self.assertIsNone(source(None))
        api.jobs[0]['started_at'] = (NOW - timedelta(days=2)).isoformat()
        self.assertIsNone(source(('lint', 'test')))
        self.assertEqual(source(('test',)), 100)

    def test_selected_lanes_follow_the_flags_and_refuse_invalid_ones(self):
        flags = {'lint': 'true', 'test': 'true', 'deny': 'false', 'harness': 'false', 'ship': 'true', 'shell': 'false'}
        self.assertEqual(ci_tree.selected_lanes(PROFILE, flags), ('lint', 'test'))
        self.assertEqual(ci_tree.selected_lanes(PROFILE, every('false')), ())
        store = ci_tree.WORKFLOWS['store']
        self.assertEqual(ci_tree.selected_lanes(store, {'run': 'true'}), store.lanes)
        self.assertEqual(ci_tree.selected_lanes(store, {'run': 'false'}), ())
        for invalid in ({**flags, 'deny': ''}, {**flags, 'deny': 'yes'}, {**flags, 'deny': True},
                        {name: value for name, value in flags.items() if name != 'harness'}, {}):
            with self.subTest(invalid=invalid):
                with self.assertRaises(ValueError):
                    ci_tree.selected_lanes(PROFILE, invalid)

    def run_main(self, gate_flags):
        """The `evidence ci` command over a source run that ran lint and test; returns its outputs."""
        api = API()
        api.jobs = source_jobs(('lint', 'test'))
        # The command reads the real clock, so the source jobs ran an hour ago.
        recent = datetime.now(timezone.utc) - timedelta(hours=1)
        for job in api.jobs:
            if 'started_at' in job:
                job.update(started_at=recent.isoformat(), completed_at=recent.isoformat())
        with tempfile.TemporaryDirectory() as directory:
            output, summary, event = (Path(directory, name) for name in ('output', 'summary', 'event.json'))
            event.write_text(json.dumps({'ref': 'refs/heads/main', 'after': REBASED}))
            env = {'GITHUB_REPOSITORY': 'owner/repo', 'GH_TOKEN': 'unit-test-placeholder',
                   'GITHUB_EVENT_NAME': 'push', 'GITHUB_EVENT_PATH': str(event),
                   'GITHUB_REPOSITORY_ID': '1', 'GITHUB_RUN_ID': '200',
                   'GITHUB_OUTPUT': str(output), 'GITHUB_STEP_SUMMARY': str(summary)}
            if gate_flags is not None:
                env['GATE_FLAGS'] = gate_flags
            with mock.patch.dict(ci_tree.os.environ, env, clear=True), \
                    mock.patch.object(ci_tree, 'GitHub', return_value=api), \
                    mock.patch.object(ci_tree, 'git_tree', return_value=TREE), \
                    mock.patch.object(sys, 'argv', ['ci_tree.py', 'evidence', 'ci']), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(ci_tree.main(), 0)
            return output.read_text()

    def test_the_command_reads_the_gate_flags_and_fails_closed(self):
        flags = {'lint': 'true', 'test': 'true', 'deny': 'false', 'harness': 'false'}
        self.assertIn('source_run=100\n', self.run_main(json.dumps(flags)))
        self.assertIn('reuse=true\n', self.run_main(json.dumps(flags)))
        for gate_flags in (json.dumps({**flags, 'deny': 'true'}), json.dumps({**flags, 'harness': ''}),
                           json.dumps({'lint': 'true'}), '{}', '[]', 'null', '', 'not json', None):
            with self.subTest(gate_flags=gate_flags):
                self.assertIn('source_run=\n', self.run_main(gate_flags))
                self.assertIn('reuse=false\n', self.run_main(gate_flags))


class RefreshTests(unittest.TestCase):
    def test_other_cache_lanes_keep_their_test_compile_selections(self):
        self.assertEqual(ci_tree.refresh_commands('test')[0].argv,
                         ('cargo', 'build', '--locked', '--workspace', '--tests'))
        with self.assertRaises(ValueError):
            ci_tree.refresh_commands('test-macos')
        for backend in ('bdb', 'kyotocabinet', 'tkrzw'):
            alternate = ci_tree.refresh_commands('store-features', backend)[0].argv
            self.assertNotIn('oxpinyin-capi', alternate)
            self.assertEqual(alternate[alternate.index('--features') + 1], backend)
        for lane, backend in [('unknown', None), ('store-features', None),
                              ('store-features', 'bad; command'), ('lint', 'tkrzw')]:
            with self.assertRaises(ValueError):
                ci_tree.refresh_commands(lane, backend)


class AggregateTests(unittest.TestCase):
    def invoke_main(self, workflow, event, outputs, results, gated_results=None, required_results=None):
        profile = ci_tree.WORKFLOWS[workflow]
        needs = {job: {'result': results[name]} for name, job in zip(profile.lanes, profile.needs)}
        outputs = {'ship': 'false', **outputs}
        for name, job, flag in profile.gated:
            needs[job] = {'result': (gated_results or {}).get(
                name, 'success' if outputs[flag] == 'true' else 'skipped')}
        needs['changes'] = {'result': 'success', 'outputs': outputs}
        needs.update({job: {'result': (required_results or {}).get(job, 'success')}
                      for job in profile.required})
        with mock.patch.object(sys, 'argv', ['ci_tree.py', 'aggregate', workflow]), \
                mock.patch.dict(ci_tree.os.environ, {'NEEDS': json.dumps(needs), 'EVENT_NAME': event}, clear=True), \
                contextlib.redirect_stdout(io.StringIO()):
            return ci_tree.main()

    def test_ci_requires_called_backends_even_when_other_lanes_skip_or_reuse(self):
        for gated, reuse in [('true', 'false'), ('false', 'false'), ('true', 'true')]:
            results = {name: 'success' if gated == 'true' and reuse == 'false' else 'skipped'
                       for name in PROFILE.lanes}
            for backend_result in ('failure', 'cancelled', 'skipped', None):
                with self.subTest(gated=gated, reuse=reuse, backend_result=backend_result):
                    self.assertEqual(self.invoke_main('ci', 'pull_request',
                        outputs_with(gated, PROFILE, reuse=reuse), results,
                        required_results={'store-backends': backend_result}), 1)
            self.assertEqual(self.invoke_main('ci', 'pull_request',
                outputs_with(gated, PROFILE, reuse=reuse), results), 0)

    def test_each_workflow_aggregate_uses_its_code_lanes(self):
        for workflow, profile in ci_tree.WORKFLOWS.items():
            for event in ('push', 'pull_request'):
                for gated in ('true', 'false'):
                    for reuse in ('true', 'false'):
                        results = {name: 'success' if gated == 'true' and (
                            reuse == 'false' or (event == 'push' and name in profile.cached)) else 'skipped'
                                   for name in profile.lanes}
                        self.assertEqual(self.invoke_main(workflow, event,
                            outputs_with(gated, profile, reuse=reuse), results), 0)
                        for name in results:
                            self.assertEqual(self.invoke_main(workflow, event,
                                outputs_with(gated, profile, reuse=reuse), {**results, **{lane: 'failure' for lane, job in zip(profile.lanes, profile.needs)
                                    if job == profile.needs[profile.lanes.index(name)]}}), 1)

    def test_full_success_and_real_failures(self):
        for profile in ci_tree.WORKFLOWS.values():
            results = dict.fromkeys(profile.lanes, 'success')
            self.assertTrue(ci_tree.aggregate('success', every('true', profile), 'false', results, profile.cached))
            for result in ('failure', 'cancelled', 'skipped', ''):
                for name in results:
                    bad = {**results, name: result}
                    self.assertFalse(ci_tree.aggregate('success', every('true', profile), 'false', bad, profile.cached))

    def test_reused_main_still_requires_every_cache_lane(self):
        for profile in ci_tree.WORKFLOWS.values():
            results = {name: 'success' if name in profile.cached else 'skipped' for name in profile.lanes}
            self.assertTrue(ci_tree.aggregate('success', every('true', profile), 'true', results, profile.cached))
            for name in profile.cached:
                self.assertFalse(ci_tree.aggregate('success', every('true', profile), 'true',
                                                  {**results, **{lane: 'failure' for lane, job in zip(profile.lanes, profile.needs)
                                    if job == profile.needs[profile.lanes.index(name)]}}, profile.cached))

    def test_reused_pr_requires_all_skipped(self):
        results = dict.fromkeys(PROFILE.lanes, 'skipped')
        self.assertTrue(ci_tree.aggregate('success', every('true'), 'true', results, PROFILE.cached, 'pull_request'))
        self.assertFalse(ci_tree.aggregate('failure', every('true'), 'true', results, PROFILE.cached, 'pull_request'))

    def test_own_gate_lane_runs_when_its_flag_is_true_even_on_a_reused_tree(self):
        results = dict.fromkeys(PROFILE.lanes, 'skipped')
        for reuse in ('true', 'false'):
            lanes = results if reuse == 'true' else dict.fromkeys(PROFILE.lanes, 'success')
            with self.subTest(reuse=reuse):
                self.assertTrue(ci_tree.aggregate('success', every('true'), reuse, lanes, PROFILE.cached, 'pull_request',
                                                  {'shipping-sizes': ('success', 'true')}))
                for bad in ('skipped', 'failure', 'cancelled', ''):
                    self.assertFalse(ci_tree.aggregate('success', every('true'), reuse, lanes, PROFILE.cached, 'pull_request',
                                                       {'shipping-sizes': (bad, 'true')}))

    def test_own_gate_lane_must_skip_when_its_flag_is_false(self):
        results = dict.fromkeys(PROFILE.lanes, 'skipped')
        self.assertTrue(ci_tree.aggregate('success', every('false'), 'false', results, PROFILE.cached, 'pull_request',
                                          {'shipping-sizes': ('skipped', 'false')}))
        for bad in ('success', 'failure', 'cancelled', ''):
            self.assertFalse(ci_tree.aggregate('success', every('false'), 'false', results, PROFILE.cached, 'pull_request',
                                               {'shipping-sizes': (bad, 'false')}))
        self.assertFalse(ci_tree.aggregate('success', every('false'), 'false', results, PROFILE.cached, 'pull_request',
                                           {'shipping-sizes': ('skipped', '')}))

    def test_invalid_gate_never_passes(self):
        results = dict.fromkeys(PROFILE.lanes, 'skipped')
        for gated, reuse in [('', 'false'), ('false', ''), ('wat', 'false')]:
            self.assertFalse(ci_tree.aggregate('success', every(gated), reuse, results, PROFILE.cached))
        missing = dict(list(every('false').items())[1:])
        self.assertFalse(ci_tree.aggregate('success', missing, 'false', results, PROFILE.cached))

    def test_each_lane_follows_its_own_flag(self):
        flags = {**every('false'), 'harness': 'true'}
        results = {**dict.fromkeys(PROFILE.lanes, 'skipped'), 'harness': 'success'}
        self.assertTrue(ci_tree.aggregate('success', flags, 'false', results, PROFILE.cached, 'pull_request'))
        for lane in PROFILE.lanes:
            with self.subTest(lane=lane):
                wrong = {**results, lane: 'skipped' if results[lane] == 'success' else 'success'}
                self.assertFalse(ci_tree.aggregate('success', flags, 'false', wrong, PROFILE.cached, 'pull_request'))


class GateTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.git('init', '-q', '-b', 'main')
        self.git('config', 'user.name', 'Shenghao Chen')
        self.git('config', 'user.email', 'shenghaoc@outlook.com')
        self.base = self.commit({'README.md': 'r\n', 'crates/oxpinyin-core/lib': 'a\n'})

    def tearDown(self):
        self.directory.cleanup()

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.root), *args], text=True).strip()

    def commit(self, files):
        for name, text in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        self.git('add', '-A')
        self.git('commit', '-q', '-m', 'change')
        return self.git('rev-parse', 'HEAD')

    def selected(self, path):
        self.git('reset', '-q', '--hard', self.base)
        self.commit({path: 'changed\n'})
        flags = ci_tree.gate(self.base, root=self.root)
        return {name for name, value in flags.items() if value}

    def test_a_missing_or_unknown_base_selects_every_job(self):
        everything = set(ci_tree.CI_INPUTS)
        for base in ('', '0' * 40, 'HEAD'):
            with self.subTest(base=base):
                self.assertEqual({name for name, value in ci_tree.gate(base, root=self.root).items() if value},
                                 everything)

    def test_paths_select_the_jobs_that_read_them(self):
        cases = {
            'vcpkg-overlays/tkrzw/pin.json': {'lint', 'test', 'harness'},
            'vcpkg-overlays/tkrzw/vcpkg.json': {'lint', 'test', 'harness'},
            'vcpkg-overlays/tkrzw/portfile.cmake': {'lint', 'test', 'harness'},
            'vcpkg-overlays/tkrzw/CMakeLists.txt': {'lint', 'test', 'harness'},
            'vcpkg-overlays/tkrzw/usage': {'lint', 'test', 'harness'},
            'docs/note.md': set(),
            'docs/findings/compatibility-policy.md': set(),
            'docs/findings/glib-libc-call-sites.tsv': {'harness'},
            'AGENTS.md': set(),
            # cells-for-diff.test.py runs the gate script in this workflow.
            '.github/workflows/store-backends.yml': {'harness'},
            '.github/workflows/verify-nightly.yml': set(),
            'tools/bisection/contract_axis_c.py': {'harness'},
            'tools/dev/ci_tree.test.py': {'harness'},
            'tools/oracle/build.py': {'lint', 'test', 'harness'},
            'tools/dev/lint-crate-cfg-placement.test.py': {'lint', 'harness'},
            'tools/dev/abi.py': {'lint', 'test', 'harness'},
            'tools/dev/new_module.py': {'lint', 'test', 'harness'},
            'tools/oracle/oracle-pin.txt': {'lint', 'test', 'harness', 'ship'},
            'tools/bisection/startup_profile.py': {'lint', 'test', 'harness'},
            'tools/bisection/bisect.c': {'lint', 'test', 'harness'},
            # A Rust-only change does not select the harness.
            'crates/oxpinyin-corpus/src/lib.rs': {'lint', 'test', 'ship'},
            'crates/oxpinyin-core/src/lib.rs': {'lint', 'test', 'ship'},
            'crates/oxpinyin-core/tests/suite.rs': {'lint', 'test', 'ship'},
            'crates/oxpinyin-core/src/table_data/mod.rs': {'lint', 'test', 'ship'},
            'crates/oxpinyin-store/examples/bdb_write_profile.rs': {'lint', 'test', 'ship'},
            'crates/oxpinyin-engine/benches/new.rs': {'lint', 'test', 'ship'},
            # The files directly in a crate's directory are harness inputs: packaging.test.py copies them.
            'crates/oxpinyin-capi/build.rs': {'lint', 'test', 'harness', 'ship'},
            'crates/oxpinyin-capi/build_pin_version.rs': {'lint', 'test', 'harness', 'ship'},
            'crates/oxpinyin-corpus/Cargo.toml': {'lint', 'test', 'deny', 'harness', 'ship'},
            'crates/oxpinyin-capi/libpinyin.ver': {'lint', 'test', 'harness', 'ship'},
            'crates/oxpinyin-zhuyin-capi/novel_types.h': {'lint', 'test', 'harness', 'ship'},
            'fixtures/w3/tkt/bigram.db': {'lint', 'test', 'harness'},
            'data/table.conf.in': {'harness'},
            'fuzz/Cargo.lock': {'lint', 'deny'},
            'fuzz/fuzz_targets/x.rs': {'lint'},
            'fuzz/rust-toolchain.toml': {'lint', 'harness'},
            'deny.toml': {'deny'},
            'clippy.toml': {'lint'},
            '.config/nextest.toml': {'test'},
            'Cargo.lock': {'lint', 'test', 'deny', 'harness', 'ship'},
            'rust-toolchain.toml': {'lint', 'test', 'deny', 'harness', 'ship'},
            '.python-version': {'lint', 'test', 'harness'},
            'justfile': {'lint', 'test', 'harness', 'ship'},
            '.github/actions/bootstrap/action.yml': {'lint', 'test', 'deny', 'harness', 'ship'},
            '.github/workflows/ci.yml': {'lint', 'test', 'deny', 'harness', 'ship'},
            'scripts/new.sh': {'lint'},
            'tools/bisection/契约.py': {'harness'},
        }
        for path, jobs in cases.items():
            with self.subTest(path=path):
                self.assertEqual(self.selected(path), jobs)
        with self.subTest(rename='crates/oxpinyin-core/lib -> docs/lib'):
            self.git('reset', '-q', '--hard', self.base)
            self.git('rm', '-q', 'crates/oxpinyin-core/lib')
            self.commit({'docs/lib': 'a\n'})
            self.assertEqual(self.git('diff', '--name-status', self.base, 'HEAD'),
                             'R100\tcrates/oxpinyin-core/lib\tdocs/lib')
            flags = ci_tree.gate(self.base, root=self.root)
            self.assertEqual({name for name, value in flags.items() if value}, {'lint', 'test', 'harness', 'ship'})

    def test_every_generated_table_selects_the_harness(self):
        from tools.dev import rust_tables
        self.assertEqual(set(ci_tree.GENERATED_TABLES), set(rust_tables.render()),
                         'GENERATED_TABLES must name exactly the files rust-tables writes')
        self.assertEqual(len(ci_tree.GENERATED_TABLES), 13)
        for path in ci_tree.GENERATED_TABLES:
            with self.subTest(path=path):
                self.assertTrue((ROOT / path).is_file())
                self.assertEqual(self.selected(path), {'lint', 'test', 'harness', 'ship'})

    def test_no_step_of_a_lane_depends_on_a_path_flag(self):
        # Evidence is read per lane: a successful `lint` in the source run stands for every
        # check of `lint`, ShellCheck included. A step that ran only for some path selection
        # would let a later run with another selection reuse a result that never ran it.
        workflow = (ROOT / PROFILE.path).read_text()
        outputs = set(re.findall(r'needs\.changes\.outputs\.(\w+)', workflow))
        self.assertEqual(outputs, {*PROFILE.flags, 'ship', 'reuse', 'tree', 'source_run'})
        self.assertEqual(set(ci_tree.CI_INPUTS), {*PROFILE.flags, 'ship'})
        steps = re.findall(r'^ {6,}(?:- )?if: (.*)$', workflow, re.MULTILINE)
        self.assertGreater(len(steps), 20)
        for condition in steps:
            with self.subTest(condition=condition):
                self.assertEqual(set(re.findall(r'needs\.changes\.outputs\.(\w+)', condition)) - {'reuse'}, set())
        shellcheck = re.search(r'- name: ShellCheck tracked scripts[^\n]*\n +if: (.*)\n', workflow)
        self.assertEqual(shellcheck[1], "needs.changes.outputs.reuse != 'true'")
        # The lint job is selected by every tracked script.
        self.assertTrue(ci_tree.CI_INPUTS['lint'].selects('tools/oracle/build-oracle.sh'))
        self.assertTrue(ci_tree.CI_INPUTS['lint'].selects('new.sh'))

    def test_event_base(self):
        self.assertEqual(ci_tree.event_base('pull_request', {'pull_request': {'base': {'sha': HEAD}}}), HEAD)
        self.assertEqual(ci_tree.event_base('push', {'before': HEAD}), HEAD)
        self.assertEqual(ci_tree.event_base('push', {}), '')


if __name__ == '__main__':
    unittest.main()
