"""The reference image tags every deps-stage input and refuses a missing Bash floor."""
from pathlib import Path
import os
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class OracleDependenciesTests(unittest.TestCase):
    def test_local_build_accepts_exact_reference_stage_copies(self):
        source = (ROOT / 'tools/oracle/image.py').read_text()
        build = source.split('\ndef build_image(', 1)[1].split('\ndef ', 1)[0]
        self.assertIn("'-f', 'tools/oracle/Dockerfile.oracle'", build)
        self.assertIn('verify_image(', build)
        # The deps tag hashes each copied input: these files, .python-version, and Dockerfile.oracle,
        # which pins the uv image by its index digest.
        pattern = (r'^COPY (tools/(oracle/(version-manifest.json|versions.py)|lib/bash-floor.sh) |\.python-version '
                   r'|--from=ghcr\.io/astral-sh/uv:[0-9.]+@sha256:[0-9a-f]{64} /uv /uvx /bin/$)')
        docker = (ROOT / 'tools/oracle/Dockerfile.oracle').read_text().split('FROM deps AS build', 1)[0]
        copies = [line for line in docker.splitlines() if line.startswith('COPY ') and '/opt/oracle-recipe/' not in line]
        self.assertTrue(copies)
        for line in copies:
            self.assertRegex(line, pattern)

    def test_builder_guards_the_floor_source_line(self):
        """Static, on every host: the guard is the command after the source line. What it does at run time is the next test."""
        lines = (ROOT / 'tools/oracle/build-oracle.sh').read_text(encoding='utf-8').splitlines()
        source = lines.index('. "${0%/*}/../lib/bash-floor.sh"')
        self.assertEqual(lines[source + 1], '[[ $? -eq 0 && -n ${OXPINYIN_BASH_FLOOR-} ]] || '
                         '{ echo "FAIL: $0: Bash floor not sourced" >&2; exit 2; }')
        self.assertLess(source, lines.index('set -euo pipefail'))

    # build-oracle.sh is a Unix builder. Windows has no Bash this repository supports: a bare `bash`
    # there is the WSL launcher, so this run-time check belongs to the hosts that build the oracle.
    @unittest.skipIf(sys.platform == 'win32', 'build-oracle.sh runs under Bash on Unix only')
    def test_missing_bash_floor_fails_closed_even_with_inherited_sentinel(self):
        with tempfile.TemporaryDirectory() as directory:
            script = Path(directory) / 'build-oracle.sh'
            script.write_bytes((ROOT / 'tools/oracle/build-oracle.sh').read_bytes())
            env = dict(os.environ, OXPINYIN_BASH_FLOOR='5.0')
            result = subprocess.run(['bash', str(script), '--help'], env=env,
                                    capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 2)
            self.assertIn('Bash floor not sourced', result.stderr)


if __name__ == '__main__':
    unittest.main()
