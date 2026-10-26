#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Focused checks of the existing campaign's size-only path."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('measure_function_order', Path(__file__).with_name('measure-function-order.py'))
measure = importlib.util.module_from_spec(spec)
spec.loader.exec_module(measure)


class StrippedSizes(unittest.TestCase):
    def test_stripping_uses_private_copies_and_preserves_every_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            libraries = {}
            for arm in ('base', 'pr'):
                for facade in measure.FACADES:
                    path = root / (arm + facade)
                    path.write_bytes(b'original source bytes')
                    libraries[arm, facade] = path

            def strip(argv, env):
                self.assertEqual(argv[:2], ['strip', '--strip-unneeded'])
                copy = Path(argv[2])
                self.assertNotIn(copy, libraries.values())
                copy.write_bytes(b'stripped')
                return b'', b''

            output = io.StringIO()
            with patch.object(measure, 'run', side_effect=strip), contextlib.redirect_stdout(output):
                measure.stripped_sizes(libraries, root, {})
            self.assertEqual(len(output.getvalue().splitlines()), 4)
            for path in libraries.values():
                self.assertEqual(path.read_bytes(), b'original source bytes')


class AllocationTotals(unittest.TestCase):
    def test_dhat_summary_requires_one_complete_pair(self):
        summary = b'==42== Total: 1,234 bytes in 56 blocks\n==42== At t-gmax: 789 bytes in 12 blocks\n'
        self.assertEqual(measure.allocation_totals(summary),
                         dict(allocated_bytes=1234, allocations=56, peak_heap_bytes=789))
        for invalid in (b'', summary + summary, summary.splitlines()[0]):
            with self.assertRaises(ValueError):
                measure.allocation_totals(invalid)


class RecordInputs(unittest.TestCase):
    def test_recording_refuses_a_changed_library_before_writing(self):
        for changed in (('pr', 'pinyin'), ('size-extra', 'zhuyin')):
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                libraries, subjects = {}, []
                for arm in (*measure.ARMS, 'size-extra'):
                    prefix = root / arm
                    subjects += ['--subject', f'{arm}={prefix}']
                    for facade in measure.FACADES:
                        library = prefix / 'lib' / f'lib{facade}.so.15.0.0'
                        library.parent.mkdir(parents=True, exist_ok=True)
                        library.write_bytes(b'original library')
                        libraries[arm, facade] = library
                for profile in measure.PROFILES:
                    fixture = root / 'profiles' / profile
                    (fixture / 'user').mkdir(parents=True)
                    (fixture / 'profile.manifest').write_text('fixture')
                bisect = root / 'bisect'
                bisect.write_bytes(b'fake worker; never executed')
                scratch = root / 'scratch'
                scratch.mkdir()
                destination = root / 'registration.jsonl.gz'
                argv = ['measure', *subjects, '--bisect', str(bisect), '--system-dir', str(root),
                        '--system-identity', hashlib.sha256(b'fixture').hexdigest(),
                        '--profiles-dir', str(root / 'profiles'), '--work-dir', str(root),
                        '--cpu', '0', '--base-checkout', str(root),
                        '--record-registration', str(destination)]

                def run(command, *args, **kwargs):
                    if command[0] == 'findmnt':
                        return b'ext4\n', b''
                    return json.dumps(dict(teardown='sample', observations=[])).encode(), b''

                def mutate(*args, **kwargs):
                    libraries[changed].write_bytes(b'changed during recording')

                with patch.object(sys, 'argv', argv), \
                        patch.object(measure, 'PAGE', 4096), \
                        patch.dict(measure.GEN, {'manifest': lambda path: 'fixture',
                            'collection_scratch': lambda path: contextlib.nullcontext(scratch),
                            'teardown_exports': lambda *args: ''}), \
                        patch.dict(measure.ORDER, {'check_arms': lambda *args, **kwargs: None}), \
                        patch.object(measure, 'run', side_effect=run), \
                        patch.object(measure.os, 'sched_getaffinity', return_value={0}, create=True), \
                        patch.object(measure.os, 'sched_setaffinity', create=True), \
                        patch.object(measure.worker, 'build', return_value=(bisect, {})), \
                        patch.object(measure.worker, 'Worker'), \
                        patch.object(measure.worker, 'userfiles', return_value={}), \
                        patch.object(measure.startup_controller, 'verify', side_effect=mutate), \
                        patch.object(measure.registration, 'git_identity', return_value={
                            'base_commit': 'a' * 40, 'pr_tree': 'b' * 40}), \
                        patch.object(measure.registration.Registration, 'write') as write, \
                        contextlib.redirect_stdout(io.StringIO()):
                    with self.assertRaisesRegex(ValueError, 'library changed during recording'):
                        measure.main()
                write.assert_not_called()
                self.assertFalse(destination.exists())


@unittest.skipUnless(os.name == 'posix', 'the profiler stream uses native POSIX descriptors')
class ProfilerStreams(unittest.TestCase):
    def test_failed_child_reports_the_tail_of_its_collected_profile(self):
        with tempfile.TemporaryDirectory() as directory:
            script = Path(directory) / 'valgrind'
            script.write_text("import os,sys\n"
                              "fd=int(sys.argv[1].split('=',1)[1])\n"
                              "os.write(fd,b'OMITTED_PROFILE_PREFIX'+b'x'*5000+b'PROFILE_FAILURE_TAIL')\n"
                              "sys.stderr.write('native failure\\n')\n"
                              "print('worker output')\n"
                              "sys.exit(7)\n")
            with self.assertRaises(RuntimeError) as caught:
                measure.run([sys.executable, script], profiler=True)
            message = str(caught.exception)
            for text in ('child exit 7', 'worker output', 'native failure', 'PROFILE_FAILURE_TAIL'):
                self.assertIn(text, message)
            self.assertNotIn('OMITTED_PROFILE_PREFIX', message)

    def test_large_profile_is_drained_separately_and_native_stderr_is_strict(self):
        with tempfile.TemporaryDirectory() as directory:
            script = Path(directory) / 'valgrind'
            script.write_text("import os,sys\n"
                              "fd=int(sys.argv[1].split('=',1)[1])\n"
                              "os.write(fd,b'profile'*20000)\n"
                              "sys.stderr.write('native\\n')\n"
                              "print('{}')\n")
            command = [sys.executable, script]
            out, profile = measure.run(command, profiler=True, native_stderr=b'native\n')
            self.assertEqual(out, b'{}\n')
            self.assertEqual(profile, b'profile' * 20000)
            with self.assertRaisesRegex(ValueError, 'native stderr'):
                measure.run(command, profiler=True, native_stderr=b'different')


if __name__ == '__main__':
    unittest.main()
