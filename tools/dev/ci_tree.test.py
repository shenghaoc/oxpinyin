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

    def test_function_order_is_not_a_full_or_reused_tree_evidence_lane(self):
        for names in (PROFILE.lanes, PROFILE.needs, PROFILE.skipped_lanes):
            self.assertNotIn('function-order-policy', names)
        for event in ('push', 'pull_request'):
            for result in (None, 'success', 'skipped'):
                with self.subTest(event=event, function_order=result):
                    api = API()
                    policy = [] if result is None else [dict(
                        name='function-order-policy', status='completed', conclusion=result)]
                    api.jobs += policy
                    self.assertEqual(self.evidence(api, event), 100)

                    api = API()
                    latest = {**run_fixture(), 'id': 101}
                    api.runs = [latest]
                    reused = [dict(name=name, status='completed', conclusion='skipped')
                              for name in PROFILE.skipped_lanes]
                    reused += [successful_job(PROFILE.aggregate),
                               successful_job('reused tree ' + TREE + ' from run 100'), *policy]
                    original_get, original_pages = api.get, api.pages
                    api.get = lambda path: run_fixture() if path == '/actions/runs/100' else original_get(path)
                    api.pages = lambda path, key=None: reused if '/runs/101/attempts/' in path else original_pages(path, key)
                    self.assertEqual(self.evidence(api, event), 100)

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

    def test_skipped_lanes_without_valid_source_never_pass(self):
        api = API()
        jobs = job_fixture()
        for job in jobs[:-2]:
            job['conclusion'] = 'skipped'
        self.assertIsNone(ci_tree.full_source(api, api.run, jobs, PROFILE, TREE, 1, 2, 200, now=NOW))

    def test_skipped_backend_matrix_uses_githubs_unexpanded_name(self):
        api = API()
        profile = ci_tree.WORKFLOWS['store']
        latest = run_fixture()
        latest.update(id=101, path=profile.path)
        jobs = [dict(name=name, status='completed', conclusion='skipped') for name in profile.skipped_lanes]
        jobs += [dict(name=profile.aggregate, status='completed', conclusion='success'),
                 dict(name='reused tree ' + TREE + ' from run 100', status='completed', conclusion='success')]
        source = run_fixture()
        source['path'] = profile.path
        original_get = api.get
        api.get = lambda path: source if path == '/actions/runs/100' else original_get(path)
        api.jobs = [successful_job(name)
                    for name in (*profile.lanes, profile.aggregate, 'tested tree ' + TREE)]
        self.assertEqual(ci_tree.full_source(api, latest, jobs, profile, TREE, 1, 2, 200, now=NOW), 100)
        jobs[0]['conclusion'] = 'failure'
        self.assertIsNone(ci_tree.full_source(api, latest, jobs, profile, TREE, 1, 2, 200, now=NOW))

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

    def test_rebase_onto_changed_parent_preserves_tree_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            def git(*args):
                return subprocess.check_output(['git', '-C', directory, *args], text=True).strip()
            git('init', '-q', '-b', 'main')
            git('config', 'user.name', 'Shenghao Chen')
            git('config', 'user.email', 'shenghaoc@outlook.com')
            Path(directory, 'base').write_text('base\n')
            git('add', 'base')
            git('commit', '-qm', 'base')
            base = git('rev-parse', 'HEAD')
            git('checkout', '-qb', 'topic')
            Path(directory, 'input').write_text('same tree\n')
            git('add', 'input')
            git('commit', '-qm', 'topic')
            original = git('rev-parse', 'HEAD')
            original_tree = git('rev-parse', 'HEAD^{tree}')
            git('checkout', '-q', 'main')
            git('commit', '--allow-empty', '-qm', 'new parent')
            git('checkout', '-q', 'topic')
            git('rebase', '--onto', 'main', base)
            self.assertNotEqual(git('rev-parse', 'HEAD'), original)
            self.assertEqual(git('rev-parse', 'HEAD^'), git('rev-parse', 'main'))
            self.assertEqual(git('rev-parse', 'HEAD^{tree}'), original_tree)


class MigratedWorkflowTests(unittest.TestCase):
    def test_function_order_is_required_by_aggregate_without_changing_its_gate(self):
        workflow = (ROOT / PROFILE.path).read_text()
        aggregate = workflow.split('\n  ci-aggregate:\n', 1)[1].split('\n  tree-evidence:', 1)[0]
        self.assertIn('[changes, lint, test, test-macos, deny, function-order-policy]', aggregate)
        policy = workflow.split('\n  function-order-policy:\n', 1)[1].split('\n  ci-aggregate:', 1)[0]
        self.assertIn("    if: ${{ github.event_name == 'pull_request' && needs.changes.outputs.function_order == 'true' }}\n", policy)
        self.assertNotIn('outputs.reuse', policy)

    def test_migrated_gates_keep_the_tree_reuse_guard(self):
        workflow = (ROOT / PROFILE.path).read_text()
        commands = {
            'C++ pinyin.h smoke gate': 'just heavy -- just runner cpp-smoke',
            'ABI export gate (development build)': 'just heavy -- just abi exports',
            'Allocator-pairing gate': 'just heavy -- just abi alloc-pairing',
            'ABI export gate (shipped build)': 'just heavy -- just abi exports --shipped',
        }
        for name, command in commands.items():
            with self.subTest(gate=name):
                step = workflow.split('      - name: ' + name + '\n', 1)[1].split('\n      - ', 1)[0]
                self.assertIn("        if: needs.changes.outputs.reuse != 'true'\n", step)
                self.assertIn('        run: ' + command + '\n', step)

    def test_workflow_script_paths_exist_and_shared_inputs_trigger_store_checks(self):
        for profile in ci_tree.WORKFLOWS.values():
            workflow = (ROOT / profile.path).read_text()
            for path in re.findall(r'\btools/(?:[\w.-]+/)*[\w.-]+\.(?:py|sh)\b', workflow):
                with self.subTest(workflow=profile.path, path=path):
                    self.assertTrue((ROOT / path).is_file(), path)
        store = (ROOT / ci_tree.WORKFLOWS['store'].path).read_text()
        paths = store.split('          paths=(\n', 1)[1].split('\n          )', 1)[0]
        for path in ('tools/dev', 'justfile', 'uv.toml', '.python-version', 'tools/install-python.sh'):
            with self.subTest(path=path):
                self.assertIn('            ' + path + '\n', paths + '\n')


class RefreshTests(unittest.TestCase):
    def test_lint_refresh_uses_clippy_all_targets_and_denies_warnings(self):
        commands = ci_tree.refresh_commands('lint')
        self.assertEqual([command.argv for command in commands], [
            ('cargo', 'clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings')])

    def test_matrix_refresh_checks_valid_package_selections_and_documents_workspace(self):
        commands = ci_tree.refresh_commands('backend-matrix')
        self.assertEqual([command.argv for command in commands[:4]], [
            ('cargo', 'check', '--locked', '-p', 'oxpinyin-store'),
            ('cargo', 'check', '--locked', '-p', 'oxpinyin-store', '--no-default-features', '--features', 'kyotocabinet'),
            ('cargo', 'check', '--locked', '-p', 'oxpinyin-store', '--no-default-features', '--features', 'tkrzw'),
            ('cargo', 'check', '--locked', '-p', 'oxpinyin-store', '--no-default-features', '--features', 'bdb')])
        self.assertEqual(len(commands), 7)
        for command, backend in zip(commands[4:], ('bdb', 'kyotocabinet', 'tkrzw')):
            self.assertEqual(command.argv, ('cargo', 'doc', '--locked', '--no-deps', '--workspace',
                                           '--no-default-features', '--features', backend))
            self.assertEqual(dict(command.environment), {'RUSTDOCFLAGS': '-D warnings'})

    def test_refresh_preserves_lane_environment_and_stops_on_compiler_failure(self):
        with mock.patch.dict(ci_tree.os.environ, {'RUSTFLAGS': '-D warnings'}, clear=True), \
                mock.patch.object(ci_tree.subprocess, 'run') as execute:
            ci_tree.refresh_cache('backend-matrix')
            self.assertEqual(execute.call_count, 7)
            for call in execute.call_args_list:
                self.assertEqual(call.kwargs['env']['RUSTFLAGS'], '-D warnings')
                self.assertTrue(call.kwargs['check'])
            self.assertEqual(execute.call_args_list[-1].kwargs['env']['RUSTDOCFLAGS'], '-D warnings')
            execute.reset_mock()
            execute.side_effect = subprocess.CalledProcessError(1, ['cargo', 'check'])
            with self.assertRaises(subprocess.CalledProcessError):
                ci_tree.refresh_cache('backend-matrix')
            self.assertEqual(execute.call_count, 1)

    def test_other_cache_lanes_keep_their_test_compile_selections(self):
        self.assertEqual(ci_tree.refresh_commands('test')[0].argv,
                         ('cargo', 'build', '--locked', '--workspace', '--tests'))
        portable = ci_tree.refresh_commands('test-macos')[0].argv
        self.assertIn('oxpinyin-capi', portable)
        self.assertEqual(portable[portable.index('--features') + 1], 'bdb')
        for backend in ('kyotocabinet', 'tkrzw'):
            alternate = ci_tree.refresh_commands('store-features', backend)[0].argv
            self.assertNotIn('oxpinyin-capi', alternate)
            self.assertEqual(alternate[alternate.index('--features') + 1], backend)
            self.assertIn('oxpinyin-word', alternate)
        for lane, backend in [('unknown', None), ('store-features', None),
                              ('store-features', 'bad; command'), ('lint', 'tkrzw')]:
            with self.assertRaises(ValueError):
                ci_tree.refresh_commands(lane, backend)


class AggregateTests(unittest.TestCase):
    def invoke_main(self, workflow, event, outputs, results):
        profile = ci_tree.WORKFLOWS[workflow]
        needs = {job: {'result': results[name]} for name, job in zip(profile.lanes, profile.needs)}
        needs['changes'] = {'result': 'success', 'outputs': outputs}
        if workflow == 'ci':
            needs['function-order-policy'] = {'result': results['function-order-policy']}
        with mock.patch.object(sys, 'argv', ['ci_tree.py', 'aggregate', workflow]), \
                mock.patch.dict(ci_tree.os.environ, {'NEEDS': json.dumps(needs), 'EVENT_NAME': event}, clear=True), \
                contextlib.redirect_stdout(io.StringIO()):
            return ci_tree.main()

    def test_function_order_uses_its_own_flag_and_event_regardless_of_run_or_reuse(self):
        for event in ('push', 'pull_request'):
            for flag in ('true', 'false'):
                expected = 'success' if event == 'pull_request' and flag == 'true' else 'skipped'
                for gated in ('true', 'false'):
                    for reuse in ('true', 'false'):
                        results = {name: 'success' if gated == 'true' and (
                            reuse == 'false' or (event == 'push' and name in PROFILE.cached)) else 'skipped'
                                   for name in PROFILE.lanes}
                        for result in ('success', 'failure', 'skipped', 'cancelled'):
                            with self.subTest(event=event, flag=flag, gated=gated, reuse=reuse, result=result):
                                actual = {**results, 'function-order-policy': result}
                                self.assertEqual(self.invoke_main('ci', event,
                                    {'run': gated, 'reuse': reuse, 'function_order': flag}, actual),
                                    0 if result == expected else 1)

    def test_function_order_missing_or_non_boolean_flag_fails_closed(self):
        results = {**dict.fromkeys(PROFILE.lanes, 'skipped'), 'function-order-policy': 'skipped'}
        for event in ('push', 'pull_request'):
            outputs = {'run': 'false', 'reuse': 'false'}
            self.assertEqual(self.invoke_main('ci', event, outputs, results), 1)
            for flag in (None, True, False, 0, 1, [], {}, '', 'yes', 'TRUE'):
                with self.subTest(event=event, flag=flag):
                    self.assertEqual(self.invoke_main('ci', event,
                        {**outputs, 'function_order': flag}, results), 1)

    def test_store_aggregate_does_not_require_function_order(self):
        profile = ci_tree.WORKFLOWS['store']
        self.assertEqual(self.invoke_main('store', 'pull_request',
            {'run': 'true', 'reuse': 'false'}, dict.fromkeys(profile.lanes, 'success')), 0)

    def test_full_success_and_real_failures(self):
        for profile in ci_tree.WORKFLOWS.values():
            results = dict.fromkeys(profile.lanes, 'success')
            self.assertTrue(ci_tree.aggregate('success', 'true', 'false', results, profile.cached))
            for result in ('failure', 'cancelled', 'skipped', ''):
                for name in results:
                    bad = {**results, name: result}
                    self.assertFalse(ci_tree.aggregate('success', 'true', 'false', bad, profile.cached))

    def test_reused_main_still_requires_every_cache_lane(self):
        for profile in ci_tree.WORKFLOWS.values():
            results = {name: 'success' if name in profile.cached else 'skipped' for name in profile.lanes}
            self.assertTrue(ci_tree.aggregate('success', 'true', 'true', results, profile.cached))
            for name in profile.cached:
                self.assertFalse(ci_tree.aggregate('success', 'true', 'true',
                                                  {**results, name: 'failure'}, profile.cached))

    def test_reused_pr_requires_all_skipped(self):
        results = dict.fromkeys(PROFILE.lanes, 'skipped')
        self.assertTrue(ci_tree.aggregate('success', 'true', 'true', results, PROFILE.cached, 'pull_request'))
        self.assertFalse(ci_tree.aggregate('failure', 'true', 'true', results, PROFILE.cached, 'pull_request'))

    def test_invalid_gate_never_passes(self):
        results = dict.fromkeys(PROFILE.lanes, 'skipped')
        for gated, reuse in [('', 'false'), ('false', ''), ('wat', 'false')]:
            self.assertFalse(ci_tree.aggregate('success', gated, reuse, results, PROFILE.cached))


if __name__ == '__main__':
    unittest.main()
