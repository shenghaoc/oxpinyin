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

    def test_readonly_modes_cannot_silently_ignore_inputs(self):
        for mode in ('--memory-smoke', '--list'):
            with self.assertRaisesRegex(ValueError, 'cannot select'):
                boundary.validate([mode, '--archive', '/unused/model'], {})

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
                identity.verify_state('b' * 16, 'a' * 64, root)

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
            with patch.object(identity, 'current_inputs', return_value=inputs), \
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

    def test_native_pointing_artifact_symlink_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            native = root / 'native'
            native.write_bytes(b'native')
            target = root / 'container'
            target.mkdir()
            link = target / 'library'
            link.symlink_to(native)
            with self.assertRaisesRegex(ValueError, 'outside container target'):
                identity.artifact_content({'library': {'path': str(link)}}, target)

    def test_export_refuses_missing_identity_and_changed_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(OSError):
                identity.verify_results(root, 'expected')
            inputs = {'schema': 1, 'environment': 'container'}
            serialized = json.dumps(inputs, sort_keys=True, indent=2) + '\n'
            expected = identity.hashlib.sha256(serialized.encode()).hexdigest()
            result = root / 'results.json'
            result.write_text('[]')
            identity.write_record(root / 'provenance.json', dict(
                schema=1, environment='container', inputs=inputs, input_sha256=expected,
                artifacts=None, artifacts_sha256=identity.digest_record(None),
                files={'results.json': identity.digest_file(result)}))
            identity.verify_results(root, expected)
            result.write_text('[1]')
            with self.assertRaisesRegex(ValueError, 'result content mismatch'):
                identity.verify_results(root, expected)


if __name__ == '__main__':
    unittest.main()
