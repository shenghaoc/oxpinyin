"""The hot-stack printer consumes hottest()'s (stack, count) rows."""

import gzip
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('extract-hot-stacks.py')


class ReportTests(unittest.TestCase):
    def test_reports_count_and_percentage_for_a_ranked_stack(self):
        profile = {
            'threads': [{
                'stringArray': ['root', 'leaf'],
                'samples': {'stack': [1, 1]},
                'stackTable': {'prefix': [None, 0], 'frame': [0, 1]},
                'frameTable': {'func': [0, 1]},
                'funcTable': {'name': [0, 1]},
            }],
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'profile.json.gz'
            with gzip.open(path, 'wt', encoding='utf-8') as handle:
                json.dump(profile, handle)
            result = subprocess.run([sys.executable, str(SCRIPT), str(path)],
                                    capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(' 1.      2  (100.0%)  leaf', result.stdout)


if __name__ == '__main__':
    unittest.main()
