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
        build = (ROOT / 'tools/oracle/build-contract-image.sh').read_text()
        self.assertIn('-f tools/oracle/Dockerfile.oracle', build)
        self.assertIn('py_oracle verify-image', build)
        pattern = r'^COPY tools/(oracle/(version-manifest.json|versions.py)|tkrzw/distro-probe.sh|lib/bash-floor.sh) '
        docker = (ROOT / 'tools/oracle/Dockerfile.oracle').read_text().split('FROM deps AS build', 1)[0]
        copies = [line for line in docker.splitlines() if line.startswith('COPY ')]
        self.assertTrue(copies)
        for line in copies:
            self.assertRegex(line, pattern)

    def test_performance_is_local_and_snapshot_uses_testing(self):
        docker = (ROOT / 'tools/oracle/Dockerfile.oracle').read_text()
        workflow = (ROOT / '.github/workflows/verify-nightly.yml').read_text()
        stable = 'debian:testing'
        self.assertIn('FROM deps AS performance', docker)
        stage = docker.split('FROM deps AS performance', 1)[1].split('\nFROM ', 1)[0]
        self.assertIn('ENV OXPINYIN_TKRZW_LIBRARY=', stage)
        for own_build in ('tools.tkrzw.build', '/opt/tkrzw', 'OXPINYIN_TKRZW_PREFIX', 'LD_LIBRARY_PATH', 'PKG_CONFIG_PATH'):
            self.assertNotIn(own_build, stage)
        snapshot = workflow.split('  perf-snapshot:', 1)[1].split('\n  test-', 1)[0]
        self.assertIn('image: ' + stable, snapshot)
        self.assertIn('OXPINYIN_MEASUREMENT_IMAGE: ' + stable, snapshot)
        self.assertNotIn('Dockerfile.oracle', snapshot.replace('# keep aligned with tools/oracle/Dockerfile.oracle', ''))
        self.assertEqual(workflow.count('image: debian:testing'), 4)
        self.assertNotIn('restore-keys: |\n            perf-snapshot-', snapshot)

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
