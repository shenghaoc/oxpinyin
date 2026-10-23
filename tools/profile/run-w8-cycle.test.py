"""W8 launch setup must guard the subject before a profiler sees the worker."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name('run-w8-cycle.sh')


class LaunchTests(unittest.TestCase):
    def launch_setup(self, status=0):
        source = SCRIPT.read_text()
        # Execute the runner's real command setup, without its unrelated
        # installation/model preparation or capture-producing profiler branches.
        setup = source[source.index('run_pinned() {'):source.index('TIMING_JSON=')]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            environment = dict(os.environ, PROFILE_DIR=str(root),
                               CAPI_SO=str(root / 'subject.so'),
                               CAPI_DATA=str(root / 'data'),
                               TKRZW_RUNTIME=str(SCRIPT.parent.parent / 'tkrzw/runtime.sh'))
            command = ('set -eu\n'
                       'tkrzw_run() { printf "GUARD %s %s %s\\n" "$1" "$2" "$3"; '
                       f'return {status}; }}\n' + setup +
                       '\nprintf "PROFILER\\n"; printf "%s\\n" "${CYCLE_CMD[@]}"\n')
            result = subprocess.run(['bash', '-c', command], env=environment,
                                    capture_output=True, text=True)
            return result, root

    def test_profiler_receives_worker_after_subject_preflight(self):
        result, root = self.launch_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(),
                         [f'GUARD auto subject {root / "subject.so"}', 'PROFILER',
                          str(root / 'bisect'), '--perf', str(root / 'subject.so'),
                          str(root / 'data')])

    def test_refusal_prevents_profiler_start(self):
        result, _ = self.launch_setup(23)
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertNotIn('PROFILER', result.stdout)


if __name__ == '__main__':
    unittest.main()
