"""The reference image installs precisely the common and three cell inputs."""
import json
from pathlib import Path
import re
import os
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class OracleDependenciesTests(unittest.TestCase):
    def test_image_packages_match_common_and_cells(self):
        groups = json.loads((ROOT / 'tools/oracle/dependencies.json').read_text())
        packages = [package for group in groups.values() for package in group]
        self.assertEqual(len(packages), len(set(packages)))
        docker = (ROOT / 'tools/oracle/Dockerfile.oracle').read_text()
        install = docker.split('install -y --no-install-recommends', 1)[1].split(';', 1)[0]
        actual = set(re.findall(r'[a-z0-9][a-z0-9+.-]*', install))
        expected = {package for group in ('common', 'bdb', 'kc', 'tkrzw') for package in groups[group]}
        self.assertEqual(actual, expected)
        self.assertTrue(set(groups['full']).isdisjoint(actual))

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

    def test_performance_is_local(self):
        docker = (ROOT / 'tools/oracle/Dockerfile.oracle').read_text()
        self.assertIn('FROM deps AS performance', docker)
        stage = docker.split('FROM deps AS performance', 1)[1].split('\nFROM ', 1)[0]
        self.assertIn('ENV OXPINYIN_TKRZW_LIBRARY=', stage)
        for own_build in ('tools.tkrzw.build', '/opt/tkrzw', 'OXPINYIN_TKRZW_PREFIX', 'LD_LIBRARY_PATH', 'PKG_CONFIG_PATH'):
            self.assertNotIn(own_build, stage)

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
