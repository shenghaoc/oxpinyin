"""The reference image tags every deps-stage input."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class OracleDependenciesTests(unittest.TestCase):
    def test_every_deps_stage_copy_is_a_hashed_input(self):
        # The deps tag hashes each copied input: these files, .python-version, and Dockerfile.oracle,
        # which pins the uv image by its index digest.
        pattern = (r'^COPY (tools/oracle/(version-manifest.json|versions.py) |\.python-version '
                   r'|--from=ghcr\.io/astral-sh/uv:[0-9.]+@sha256:[0-9a-f]{64} /uv /uvx /bin/$)')
        docker = (ROOT / 'tools/oracle/Dockerfile.oracle').read_text().split('FROM deps AS build', 1)[0]
        copies = [line for line in docker.splitlines() if line.startswith('COPY ') and '/opt/oracle-recipe/' not in line]
        self.assertTrue(copies)
        for line in copies:
            self.assertRegex(line, pattern)

if __name__ == '__main__':
    unittest.main()
