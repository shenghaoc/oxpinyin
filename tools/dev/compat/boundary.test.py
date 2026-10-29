"""Light boundary/identity harness. No runtime, compiler or network is invoked."""
import io
import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev.compat import boundary, identity

IMAGE, SOURCE, RUN = 'b' * 16, 'a' * 64, '20261021T000000Z-1'
PYTHON = '/opt/uv/python/cpython-3.15.0-linux-x86_64-gnu/bin/python3.15'


def input_identity(inputs):
    return identity.hashlib.sha256((json.dumps(inputs, sort_keys=True, indent=2) + '\n').encode()).hexdigest()


class BoundaryTests(unittest.TestCase):
    def test_native_state_and_unknown_inputs_fail_before_launch(self):
        for name in ('CARGO_HOME', 'CARGO_TARGET_DIR', 'UV_CACHE_DIR',
                     'UV_PYTHON_INSTALL_DIR', 'OXPINYIN_TARGET_ROOT',
                     'OXPINYIN_CAPI_SO', 'PINYIN_ORACLE_PREFIX',
                     'OXPINYIN_MISSPELLED_INPUT', 'UV_PYTHON_VARIANT'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                boundary.validate(['--memory-smoke'], {name: '/native/state'})

    def test_native_budget_override_is_host_only_and_absolute(self):
        boundary.validate(['--memory-smoke'], {'OXPINYIN_LOCK_DIR': '/native/locks'})
        with self.assertRaisesRegex(ValueError, 'absolute'):
            boundary.validate(['--memory-smoke'], {'OXPINYIN_LOCK_DIR': 'relative'})

    def test_unknown_option_and_unknown_runner_are_refused(self):
        with self.assertRaises(SystemExit):
            boundary.validate(['--target-root', '/native/target', '--memory-smoke'], {})
        with self.assertRaisesRegex(ValueError, 'unknown runner'):
            boundary.validate(['--runners', 'not-a-registered-runner'], {})

    def test_timeout_cannot_name_unselected_work(self):
        with self.assertRaisesRegex(ValueError, 'selected runner'):
            boundary.validate(['--runners', 'pc-version', '--runner-timeout', 'bisect=1'], {})

    def test_invalid_timeout_returns_a_diagnostic_without_a_traceback(self):
        with patch.dict(os.environ, {}, clear=True), patch('sys.stderr', new_callable=io.StringIO) as error:
            self.assertEqual(boundary.main(['--runners', 'pc-version', '--runner-timeout', 'pc-version=zero']), 2)
            self.assertIn('positive integer', error.getvalue())
            self.assertNotIn('Traceback', error.getvalue())

    def test_contract_only_issue_tags_remain_valid_inputs(self):
        with patch.object(boundary, 'contract_issues', return_value={'contract-only-tag'}):
            boundary.validate(['--runners', 'contract-diff', '--issue', 'contract-only-tag'], {})
            with self.assertRaisesRegex(ValueError, 'unknown issue'):
                boundary.validate(['--runners', 'contract-diff', '--issue', 'not-known'], {})

    def test_cases_need_contract_diff_in_runners(self):
        for argv in (['--runners', 'pc-version', '--cases', 'default-option-parse'],
                     ['--issue', '919', '--cases', 'default-option-parse'],
                     ['--list', '--cases', 'default-option-parse']):
            with self.subTest(argv=argv), self.assertRaisesRegex(ValueError, 'requires --runners contract-diff'):
                boundary.validate(argv, {})
        args = boundary.validate(['--runners', 'pc-version,contract-diff', '--cases', 'default-option-parse'], {})
        self.assertEqual(args.cases, 'default-option-parse')

    def test_readonly_modes_cannot_silently_ignore_inputs(self):
        for mode in ('--memory-smoke', '--list'):
            with self.assertRaisesRegex(ValueError, 'cannot select'):
                boundary.validate([mode, '--archive', '/unused/model'], {})

    def test_prefixes_require_contract_diff_and_are_preserved(self):
        for argv in (['--runners', 'pc-version'], ['--list'], ['--issue', '919']):
            with self.subTest(argv=argv), self.assertRaisesRegex(
                    ValueError, '--cases-prefix requires --runners contract-diff'):
                boundary.validate([*argv, '--cases-prefix', 'table-conf-'], {})
        args = boundary.validate(['--runners', 'contract-diff', '--cases-prefix', 'table-conf-,user-conf-'], {})
        self.assertEqual(args.cases_prefix, 'table-conf-,user-conf-')

    def test_wrong_model_fails_without_runtime(self):
        with tempfile.TemporaryDirectory() as directory:
            model = Path(directory) / 'model-input'
            model.write_bytes(b'not the frozen model')
            with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
                boundary.validate(['--runners', 'pc-version', '--archive', str(model)], {})


def archive(entries):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode='w') as output:
        for name, payload, link in entries:
            member = tarfile.TarInfo(name)
            member.mode = 0o644
            if link:
                member.type, member.linkname = tarfile.SYMTYPE, payload
                output.addfile(member)
            else:
                member.size = len(payload)
                output.addfile(member, io.BytesIO(payload))
    stream.seek(0)
    return stream


class IdentityTests(unittest.TestCase):
    def test_result_and_inner_state_directories_cannot_redirect_to_native(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            native = root / 'native'
            native.mkdir()
            (root / '.oxpinyin-container').mkdir()
            (root / '.oxpinyin-container/results').symlink_to(native)
            with self.assertRaisesRegex(ValueError, 'redirects'):
                identity.result_directory(root, 'a' * 64, '20261021T000000Z-1')
            (root / 'work').symlink_to(native)
            with self.assertRaisesRegex(ValueError, 'redirects'):
                identity.verify_state(IMAGE, SOURCE, 'bdb', RUN, root)

    def test_symlinked_ancestor_is_trusted_for_host_and_container_namespaces(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            real = root / 'real'
            (real / 'repo').mkdir(parents=True)
            (real / 'state').mkdir()
            alias = root / 'alias'
            alias.symlink_to(real, target_is_directory=True)
            repo = alias / 'repo'
            with patch.object(boundary.Path, 'cwd', return_value=repo):
                boundary.validate(['--memory-smoke'], {})
            self.assertEqual(
                identity.result_directory(repo, 'a' * 64, '20261021T000000Z-1'),
                real.resolve() / 'repo/.oxpinyin-container/results' / ('a' * 64) / '20261021T000000Z-1')
            identity.verify_state(IMAGE, SOURCE, 'bdb', RUN, alias / 'state')

    def test_symlinks_below_trusted_base_still_fail_with_symlinked_ancestor(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            real = root / 'real'
            real.mkdir()
            alias = root / 'alias'
            alias.symlink_to(real, target_is_directory=True)
            native = root / 'native'
            native.mkdir()
            (real / '.oxpinyin-container').symlink_to(native, target_is_directory=True)
            with patch.object(boundary.Path, 'cwd', return_value=alias):
                with self.assertRaisesRegex(ValueError, '^container result namespace must not redirect through a symlink$'):
                    boundary.validate(['--memory-smoke'], {})
            (real / '.oxpinyin-container').unlink()
            (real / '.oxpinyin-container').mkdir()
            (real / '.oxpinyin-container/results').symlink_to(native, target_is_directory=True)
            with self.assertRaisesRegex(ValueError, '^container namespace redirects through a symlink:'):
                identity.result_directory(alias, 'a' * 64, '20261021T000000Z-1')
            (real / 'work').symlink_to(native, target_is_directory=True)
            with self.assertRaisesRegex(ValueError, '^container namespace redirects through a symlink:'):
                identity.verify_state(IMAGE, SOURCE, 'bdb', RUN, alias)

    def test_cell_and_run_directories_cannot_redirect_before_any_write(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'state'
            native = Path(directory) / 'native'
            native.mkdir()
            identity.verify_state(IMAGE, SOURCE, 'bdb', RUN, root)
            cell = root / f'container/{IMAGE}/targets/{SOURCE}/bdb'
            cell.parent.mkdir(parents=True)
            cell.symlink_to(native, target_is_directory=True)
            with self.assertRaisesRegex(ValueError, '^container namespace redirects through a symlink:'):
                identity.verify_state(IMAGE, SOURCE, 'bdb', RUN, root)
            cell.unlink()
            run = root / f'results/container/{SOURCE}/{RUN}'
            run.parent.mkdir(parents=True)
            run.symlink_to(native, target_is_directory=True)
            with self.assertRaisesRegex(ValueError, '^container namespace redirects through a symlink:'):
                identity.verify_state(IMAGE, SOURCE, 'bdb', RUN, root)
            self.assertEqual(list(native.iterdir()), [])
            for cell_name, run_id in (('lmdb', RUN), ('bdb', '../native')):
                with self.subTest(cell=cell_name, run=run_id), \
                        self.assertRaisesRegex(ValueError, 'invalid container state namespace'):
                    identity.verify_state(IMAGE, SOURCE, cell_name, run_id, root)
        inside = (ROOT / 'tools/dev/compat/guest.py').read_text()
        check = inside.index('identity.verify_state(image, source, cell, run_id)')
        self.assertLess(check, inside.index('.mkdir('))

    def test_symlinked_ancestor_is_trusted_for_artifact_targets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            real = root / 'real'
            (real / 'state/container/image/targets/source/bdb').mkdir(parents=True)
            alias = root / 'alias'
            alias.symlink_to(real, target_is_directory=True)
            target = alias / 'state/container/image/targets/source'
            artifact = target / 'bdb/library'
            artifact.write_bytes(b'library')
            inputs = dict(source_sha256='source', image='image', reference='reference', files={})
            record = dict(worktree=str(root), cell='bdb', profile='dev', toolchain={},
                          artifacts={'pinyin-capi': {'path': str(artifact)}})
            with patch.object(identity, 'STATE_ROOT', alias / 'state'), \
                    patch.object(identity, 'current_inputs', return_value=inputs), \
                    patch.dict(os.environ, OXPINYIN_TARGET_ROOT=str(target)):
                record['container'] = identity.seal_artifacts(record, target / 'bdb')
                identity.verify_artifacts(record, root, {'pinyin-capi': str(artifact)})

    def test_symlinks_below_trusted_base_still_fail_for_artifact_targets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            real = root / 'real'
            (real / 'state/container/image').mkdir(parents=True)
            native = root / 'native'
            native.mkdir()
            (real / 'state/container/image/targets').symlink_to(native, target_is_directory=True)
            alias = root / 'alias'
            alias.symlink_to(real, target_is_directory=True)
            target = alias / 'state/container/image/targets/source'
            inputs = dict(source_sha256='source', image='image', reference='reference', files={})
            sealed = dict(source_sha256='source', image='image', reference='reference',
                          cell='bdb', profile='dev', toolchain={}, artifacts={})
            record = dict(cell='bdb', profile='dev', toolchain={}, container=sealed)
            with patch.object(identity, 'STATE_ROOT', alias / 'state'), \
                    patch.object(identity, 'current_inputs', return_value=inputs), \
                    patch.dict(os.environ, OXPINYIN_TARGET_ROOT=str(target)):
                with self.assertRaisesRegex(ValueError, '^container namespace redirects through a symlink:'):
                    identity.artifact_content({}, target / 'bdb')
                with self.assertRaisesRegex(ValueError, '^container namespace redirects through a symlink:'):
                    identity.verify_artifacts(record, root, {})

    def test_source_identity_hashes_bytes_not_size_or_mtime(self):
        first = identity.archive_content(archive([('source', b'aaaa', False)]))
        second = identity.archive_content(archive([('source', b'bbbb', False)]))
        self.assertNotEqual(identity.digest_record(first), identity.digest_record(second))

    def test_source_rejects_escaping_and_ambiguous_members(self):
        cases = [('../escape', b'', False), ('/absolute', b'', False),
                 ('link', '../outside', True), ('.git/config', b'', False)]
        for entry in cases:
            with self.subTest(entry=entry), self.assertRaises(ValueError):
                identity.archive_content(archive([entry]))
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            identity.archive_content(archive([('a', b'a', False), ('a', b'b', False)]))
        with self.assertRaisesRegex(ValueError, 'non-directory'):
            identity.archive_content(archive([('dir', 'elsewhere', True), ('dir/file', b'x', False)]))

    def test_artifact_mutation_with_unchanged_size_and_mtime_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifact = root / 'library'
            artifact.write_bytes(b'original')
            stat = artifact.stat()
            inputs = dict(source_sha256='source', image='image', reference='reference', files={})
            record = dict(worktree=str(root), cell='bdb', profile='dev', toolchain={},
                          artifacts={'pinyin-capi': {'path': str(artifact)}})
            with patch.object(identity, 'STATE_ROOT', root), \
                    patch.object(identity, 'current_inputs', return_value=inputs), \
                    patch.dict(os.environ, OXPINYIN_TARGET_ROOT=str(root)):
                record['container'] = identity.seal_artifacts(record, root)
                identity.verify_artifacts(record, root, {'pinyin-capi': str(artifact)})
                artifact.write_bytes(b'modified')
                os.utime(artifact, ns=(stat.st_atime_ns, stat.st_mtime_ns))
                with self.assertRaisesRegex(ValueError, 'content mismatch'):
                    identity.verify_artifacts(record, root, {'pinyin-capi': str(artifact)})

    def test_missing_or_wrong_source_identity_is_refused(self):
        inputs = dict(source_sha256='expected', image='image', reference='reference', files={})
        with patch.object(identity, 'current_inputs', return_value=inputs):
            with self.assertRaisesRegex(ValueError, 'missing container artifact'):
                identity.verify_artifacts({}, '.', {})
            with self.assertRaisesRegex(ValueError, 'source_sha256 mismatch'):
                identity.verify_artifacts({'container': {'source_sha256': 'stale'}}, '.', {})

    def test_unsealed_empty_claim_exports_failure_but_no_artifact_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result, target = root / 'result', root / 'targets'
            (result / 'logs').mkdir(parents=True)
            (result / 'logs/build-bdb.log').write_text('error: could not compile\n')
            (target / 'bdb').mkdir(parents=True)
            # The claim cell-artifacts.sh writes before cargo runs.
            claim = target / 'bdb/oxpinyin-cell-identity.json'
            identity.write_record(claim, dict(schema=1, worktree=str(root), cell='bdb', feature='bdb',
                                              profile='dev', opt_level='1', artifacts={}))
            inputs = dict(schema=1, environment='container', image=IMAGE, source_sha256=SOURCE, files={})
            expected = input_identity(inputs)
            state = dict(identity.state_paths(inputs), executable=PYTHON)

            def seal(status):
                (result / 'results.json').write_text(json.dumps([dict(runner='cpp-smoke', cell='bdb', status=status)]))
                identity.seal_results(result, target, 'bdb')

            with patch.object(identity, 'current_inputs', return_value=inputs), \
                    patch.object(identity, 'inner_state', return_value=state), \
                    patch.dict(os.environ, OXPINYIN_COMPAT_INPUT_SHA256=expected):
                seal('FAIL')
                self.assertIsNone(identity.read_record(result / 'provenance.json')['artifacts'])
                identity.verify_results(result, expected)
                (result / 'provenance.json').unlink()
                with self.assertRaisesRegex(ValueError, 'passing result lacks container artifact identity'):
                    seal('PASS')
                identity.write_record(claim, dict(identity.read_record(claim), artifacts={
                    'pinyin-capi': {'path': str(target / 'bdb/debug/libpinyin_capi.so')}}))
                with self.assertRaisesRegex(ValueError, 'missing container artifact content identity'):
                    seal('FAIL')

    def test_native_pointing_artifact_symlink_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            native = root / 'native'
            native.write_bytes(b'native')
            target = root / 'container'
            target.mkdir()
            link = target / 'library'
            link.symlink_to(native)
            with patch.object(identity, 'STATE_ROOT', root), \
                    self.assertRaisesRegex(ValueError, 'outside container target'):
                identity.artifact_content({'library': {'path': str(link)}}, target)

    def test_export_refuses_missing_identity_and_changed_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(OSError):
                identity.verify_results(root, 'expected')
            inputs = {'schema': 1, 'environment': 'container', 'image': IMAGE, 'source_sha256': SOURCE}
            serialized = json.dumps(inputs, sort_keys=True, indent=2) + '\n'
            expected = identity.hashlib.sha256(serialized.encode()).hexdigest()
            result = root / 'results.json'
            result.write_text('[]')
            provenance = dict(
                schema=1, environment='container', inputs=inputs, input_sha256=expected,
                artifacts=None, artifacts_sha256=identity.digest_record(None),
                files={'results.json': identity.digest_file(result)})
            identity.write_record(root / 'provenance.json', provenance)
            with self.assertRaisesRegex(ValueError, 'state mismatch'):
                identity.verify_results(root, expected)
            identity.write_record(root / 'provenance.json', dict(
                provenance, state=dict(identity.state_paths(inputs), executable=PYTHON)))
            identity.verify_results(root, expected)
            result.write_text('[1]')
            with self.assertRaisesRegex(ValueError, 'result content mismatch'):
                identity.verify_results(root, expected)

    def test_export_refuses_missing_or_altered_container_state(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            inputs = {'schema': 1, 'environment': 'container', 'image': IMAGE, 'source_sha256': SOURCE}
            expected = input_identity(inputs)
            (root / 'results.json').write_text('[]')
            provenance = dict(schema=1, environment='container', inputs=inputs, input_sha256=expected,
                              artifacts=None, artifacts_sha256=identity.digest_record(None),
                              files={'results.json': identity.digest_file(root / 'results.json')})
            state = dict(identity.state_paths(inputs), executable=PYTHON)
            cases = {
                'missing': None,
                'not a record': 'state',
                'altered path': dict(state, HOME=f'/state/container/{"c" * 16}/home'),
                'missing path': {key: value for key, value in state.items() if key != 'OXPINYIN_LOCK_DIR'},
                'extra path': dict(state, CARGO_TARGET_DIR='/native/target'),
                'native interpreter': dict(state, executable='/usr/bin/python3'),
                'escaping interpreter': dict(state, executable='/opt/uv/python/../../usr/bin/python3'),
                'relative interpreter': dict(state, executable='opt/uv/python/bin/python3'),
            }
            for name, value in cases.items():
                record = dict(provenance) if value is None else dict(provenance, state=value)
                identity.write_record(root / 'provenance.json', record)
                with self.subTest(name), self.assertRaisesRegex(ValueError, '^exported container state mismatch$'):
                    identity.verify_results(root, expected)
            identity.write_record(root / 'provenance.json', dict(provenance, state=state))
            identity.verify_results(root, expected)


if __name__ == '__main__':
    unittest.main()
