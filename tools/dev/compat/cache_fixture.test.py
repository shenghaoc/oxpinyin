"""Real Cargo checks for stable compatibility targets and interrupted reconciliation."""
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev.compat import guest, identity


class StableTargetCargoTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source, self.work, self.target = (self.root / name for name in ('source', 'work', 'target'))
        self.source.mkdir(); self.work.mkdir(); (self.source / 'src').mkdir()
        (self.source / 'Cargo.toml').write_text('[package]\nname="stable-fixture"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nfixture-support={path="support"}\n[features]\nbdb=[]\ntkrzw=[]\n')
        (self.source / 'Cargo.lock').write_text('version = 4\n\n[[package]]\nname = "fixture-support"\nversion = "0.1.0"\n\n[[package]]\nname = "stable-fixture"\nversion = "0.1.0"\ndependencies = [\n "fixture-support",\n]\n')
        (self.source / 'support/src').mkdir(parents=True)
        (self.source / 'support/Cargo.toml').write_text('[package]\nname="fixture-support"\nversion="0.1.0"\nedition="2021"\n')
        (self.source / 'support/src/lib.rs').write_text('pub fn suffix() -> &\'static str { "!" }\n')
        (self.source / 'src/main.rs').write_text('mod extra; fn main(){println!("{}{}{}", include_str!("value"), fixture_support::suffix(), extra::suffix());}\n')
        (self.source / 'src/extra.rs').write_text('pub fn suffix() -> &\'static str { "x" }\n')
        (self.source / 'src/value').write_text('old')
        self.archive = self.root / 'source.tar'
        self.env = dict(os.environ, CARGO_TARGET_DIR=str(self.target), CARGO_NET_OFFLINE='true')

    def snapshot(self):
        with tarfile.open(self.archive, 'w') as output:
            for path in sorted(self.source.rglob('*')):
                output.add(path, arcname=str(path.relative_to(self.source)), recursive=False)
        return identity.archive_content(self.archive)

    def prepare(self):
        files = self.snapshot()
        marker = guest.prepare_source_target(self.target)
        guest.remove_empty_runner_dirs(self.work, files)
        guest.reconcile(self.archive, self.work, files, identity)
        return marker

    def build(self, feature='tkrzw', check=True):
        return subprocess.run(['cargo', 'build', '--offline', '--locked', '--color', 'never', '-v', '--features', feature],
                              cwd=self.work, env=self.env, capture_output=True, text=True, check=check)

    def complete(self):
        marker = self.prepare()
        result = self.build()
        guest.finish_source_target(marker, 0)
        return result

    def output(self):
        return subprocess.check_output([self.target / 'debug/stable-fixture'], text=True).strip()

    def test_unchanged_second_run_is_fresh(self):
        self.complete()
        (self.work / 'target/runner-locks').mkdir(parents=True)
        self.assertIn('Fresh stable-fixture', self.complete().stderr)

    def test_equal_and_older_content_changes_rebuild(self):
        self.complete()
        source = self.source / 'src/value'
        for offset, value in ((0, 'new'), (-1_000_000_000, 'newer')):
            timestamp = (self.work / 'src/value').stat().st_mtime_ns
            source.write_text(value)
            os.utime(source, ns=(timestamp + offset, timestamp + offset))
            log = self.complete().stderr
            self.assertIn('Compiling stable-fixture', log)
            self.assertIn('Fresh fixture-support', log)
            self.assertEqual(self.output(), value + '!x')

    def test_deleted_source_rebuilds_and_reuses_dependency(self):
        self.complete()
        (self.source / 'src/extra.rs').unlink()
        (self.source / 'src/main.rs').write_text('fn main(){println!("{}{}", include_str!("value"), fixture_support::suffix());}\n')
        log = self.complete().stderr
        self.assertIn('Compiling stable-fixture', log)
        self.assertIn('Fresh fixture-support', log)
        self.assertEqual(self.output(), 'old!')

    def test_deleted_included_file_triggers_rebuild_failure(self):
        self.complete()
        (self.source / 'src/value').unlink()
        marker = self.prepare()
        result = self.build(check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Compiling stable-fixture', result.stderr)
        self.assertIn('Fresh fixture-support', result.stderr)
        self.assertTrue(marker.exists())

    def test_completed_failing_test_reuses_unaffected_crates(self):
        (self.source / 'src/main.rs').write_text(
            '#[test] fn fails() { assert!(false); }\nfn main() { fixture_support::suffix(); }\n')
        def test_run():
            marker = self.prepare()
            result = subprocess.run(['cargo', 'test', '--offline', '--locked', '--color', 'never', '-v'],
                                    cwd=self.work, env=self.env, capture_output=True, text=True)
            guest.finish_source_target(marker, result.returncode)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('fails ... FAILED', result.stdout)
            return result.stderr
        self.assertIn('Compiling fixture-support', test_run())
        second = test_run()
        self.assertIn('Fresh fixture-support', second)
        self.assertIn('Fresh stable-fixture', second)
        self.assertNotIn('Compiling ', second)

    def test_docs_only_change_reuses_both_crates(self):
        (self.source / 'docs').mkdir()
        (self.source / 'docs/readme.md').write_text('before')
        self.complete()
        (self.source / 'docs/readme.md').write_text('after')
        log = self.complete().stderr
        self.assertIn('Fresh stable-fixture', log)
        self.assertIn('Fresh fixture-support', log)
        self.assertNotIn('Compiling ', log)

    def test_manifest_change_rebuilds(self):
        self.complete()
        manifest = self.source / 'Cargo.toml'
        manifest.write_text(manifest.read_text() + '\n[profile.dev]\nopt-level=1\n')
        self.assertIn('Compiling stable-fixture', self.complete().stderr)

    def test_backend_feature_changes_target_namespace(self):
        common = dict(image_content='a'*64, target_triple='aarch64-unknown-linux-gnu')
        with self.assertRaisesRegex(ValueError, 'environment identity'):
            identity.target_relative(dict(common, backend='bdb', build_profile='release'))
        self.assertNotEqual(identity.target_relative(dict(common, backend='bdb')),
                            identity.target_relative(dict(common, backend='tkrzw')))
        self.assertNotEqual(identity.target_relative(dict(common, backend='tkrzw')),
                            identity.target_relative(dict(common, image_content='b'*64, backend='tkrzw')))
        self.prepare()
        for backend in ('bdb', 'tkrzw'):
            target = self.root / identity.target_relative(dict(common, backend=backend))
            self.env['CARGO_TARGET_DIR'] = str(target)
            self.assertIn('Compiling stable-fixture', self.build(backend).stderr)
            self.assertTrue((target / 'debug/stable-fixture').is_file())

    def test_interruption_before_and_after_marker(self):
        self.complete()
        # Before marker: no mutation occurred and the completed cache stays fresh.
        self.assertIn('Fresh stable-fixture', self.complete().stderr)
        source = self.source / 'src/value'
        timestamp = (self.work / 'src/value').stat().st_mtime_ns
        source.write_text('after')
        os.utime(source, ns=(timestamp - 1_000_000_000, timestamp - 1_000_000_000))
        files = self.snapshot()
        guest.prepare_source_target(self.target)
        guest.reconcile(self.archive, self.work, files, identity)
        # Simulate interruption after the marker and reconciliation, before build.
        marker = guest.prepare_source_target(self.target)
        self.assertFalse((self.target / 'debug/stable-fixture').exists())
        self.assertIn('Compiling stable-fixture', self.build().stderr)
        guest.finish_source_target(marker, 0)
        self.assertEqual(self.output(), 'after!x')


if __name__ == '__main__':
    unittest.main()
